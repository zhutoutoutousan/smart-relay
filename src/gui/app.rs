use eframe::egui;
use crate::gui::theme::CyberpunkTheme;
use crate::gui::subscription;
use crate::gui::endpoint_test;
use crate::sys_proxy::{ProxyMode, ProxyConfig, set_system_proxy};
use crate::proxy::xray_check;
use crate::proxy::core_download;
use crate::proxy::xray_adapter::{XrayAdapter, EndpointConfig};
use std::sync::{mpsc, Arc};
use std::time::Duration;
use tokio::runtime::Runtime;

pub struct App {
    api_url: String,
    health_status: HealthStatus,
    endpoints: Vec<EndpointDisplay>,
    current_connection: Option<ConnectionInfo>,
    ai_goal: String,
    ai_response: Option<String>,
    ai_loading: bool,
    theme: CyberpunkTheme,
    
    // Subscription management
    subscription_url: String,
    subscriptions: Vec<SubscriptionInfo>,
    
    // Port configuration
    local_port: u16,
    socks_port: u16,
    http_port: u16,
    
    // Xray-specific settings
    xray_settings: XraySettings,
    
    // Client parameters
    client_params: ClientParams,
    
    // System proxy configuration
    proxy_mode: ProxyMode,
    proxy_exceptions: String,
    
    // UI state
    selected_tab: Tab,
    show_add_endpoint: bool,
    editing_endpoint: Option<usize>,
    
    // Subscription loading state
    subscription_loading: bool,
    subscription_error: Option<String>,
    subscription_success: Option<String>,
    subscription_status: String, // Detailed status message
    
    // Async subscription task results (processed each frame)
    pending_subscription_result: Option<Result<(String, Vec<subscription::ParsedEndpoint>), String>>,
    
    // Tokio runtime for async operations
    runtime: Option<Arc<Runtime>>,
    
    // Channel receiver for subscription results
    subscription_receiver: Option<mpsc::Receiver<Result<(String, Vec<subscription::ParsedEndpoint>), String>>>,
    
    // Font configuration flag
    fonts_configured: bool,
    
    // Endpoint testing state
    testing_endpoints: std::collections::HashSet<String>, // IDs of endpoints being tested
    test_results: std::collections::HashMap<String, Result<u64, String>>, // Test results: latency or error
    
    // Test result receiver for processing in update loop
    test_result_receiver: Option<mpsc::Receiver<(String, endpoint_test::TestResult)>>,
    
    // Right-click menu state
    right_click_menu: Option<(egui::Pos2, usize)>, // Position and endpoint index
    
    // Connection state
    connected_endpoint_id: Option<String>, // ID of currently connected endpoint
    
    // Diagnostics window state
    show_diagnostics: Option<usize>, // Endpoint index to show diagnostics for
    
    // Core download state
    downloading_xray: bool,
    downloading_sing_box: bool,
    xray_download_status: String,
    sing_box_download_status: String,
    xray_download_error: Option<String>,
    sing_box_download_error: Option<String>,
    xray_download_receiver: Option<mpsc::Receiver<Result<std::path::PathBuf, String>>>,
    sing_box_download_receiver: Option<mpsc::Receiver<Result<std::path::PathBuf, String>>>,
    
    // Xray adapter instance
    xray_adapter: Option<Arc<XrayAdapter>>,
}

impl App {
    pub fn configure_fonts(ctx: &egui::Context) {
        // Try to configure fonts manually as fallback if egui-chinese-font didn't work
        // This attempts to use system fonts on Windows
        #[cfg(windows)]
        {
            use egui::{FontData, FontFamily, FontDefinitions};
            use std::path::Path;
            
            // Try to load common Windows Chinese fonts from system
            let font_paths = vec![
                r"C:\Windows\Fonts\msyh.ttc",      // Microsoft YaHei
                r"C:\Windows\Fonts\msyhbd.ttc",    // Microsoft YaHei Bold
                r"C:\Windows\Fonts\simsun.ttc",    // SimSun
                r"C:\Windows\Fonts\simhei.ttf",    // SimHei
            ];
            
            let mut fonts = FontDefinitions::default();
            let mut font_added = false;
            
            for font_path in font_paths {
                if Path::new(font_path).exists() {
                    match std::fs::read(font_path) {
                        Ok(font_bytes) => {
                            fonts.font_data.insert(
                                "chinese_font".to_owned(),
                                FontData::from_owned(font_bytes),
                            );
                            fonts.families
                                .entry(FontFamily::Proportional)
                                .or_default()
                                .insert(0, "chinese_font".to_owned());
                            font_added = true;
                            tracing::info!("Loaded Chinese font from: {}", font_path);
                            break;
                        }
                        Err(e) => {
                            tracing::debug!("Failed to read font {}: {}", font_path, e);
                        }
                    }
                }
            }
            
            if font_added {
                ctx.set_fonts(fonts);
            } else {
                tracing::warn!("No Chinese fonts found in system, Chinese characters may not display correctly");
            }
        }
    }
}

impl Default for App {
    fn default() -> Self {
        Self {
            api_url: "http://127.0.0.1:8080".to_string(),
            health_status: HealthStatus::Unknown,
            endpoints: vec![],
            current_connection: None,
            ai_goal: String::new(),
            ai_response: None,
            ai_loading: false,
            theme: CyberpunkTheme::default(),
            subscription_url: String::new(),
            subscriptions: vec![],
            local_port: 10808,
            socks_port: 1080,
            http_port: 8888, // Changed from 8080 to avoid conflict with Docker Desktop on Windows
            xray_settings: XraySettings::default(),
            client_params: ClientParams::default(),
            selected_tab: Tab::Overview,
            show_add_endpoint: false,
            editing_endpoint: None,
            subscription_loading: false,
            subscription_error: None,
            subscription_success: None,
            subscription_status: String::new(),
            pending_subscription_result: None,
            runtime: None,
            subscription_receiver: None,
            proxy_mode: ProxyMode::Unchanged,
            proxy_exceptions: String::new(),
            fonts_configured: false,
            testing_endpoints: std::collections::HashSet::new(),
            test_results: std::collections::HashMap::new(),
            test_result_receiver: None,
            right_click_menu: None,
            connected_endpoint_id: None,
            show_diagnostics: None,
            downloading_xray: false,
            downloading_sing_box: false,
            xray_download_status: String::new(),
            sing_box_download_status: String::new(),
            xray_download_error: None,
            sing_box_download_error: None,
            xray_download_receiver: None,
            sing_box_download_receiver: None,
            xray_adapter: None,
        }
    }
}

impl Default for XraySettings {
    fn default() -> Self {
        Self {
            log_level: "info".to_string(),
            dns_servers: vec!["8.8.8.8".to_string(), "1.1.1.1".to_string()],
            routing_domain_strategy: "AsIs".to_string(),
            enable_sniffing: true,
            enable_fragment: false,
        }
    }
}

impl Default for ClientParams {
    fn default() -> Self {
        Self {
            system_proxy: false,
            auto_start: false,
            start_minimized: false,
            allow_lan: false,
            local_dns: false,
        }
    }
}

#[derive(Default, Clone, PartialEq)]
enum Tab {
    #[default]
    Overview,
    Endpoints,
    Subscriptions,
    Configuration,
    Xray,
    AI,
}

#[derive(Default, Clone)]
pub enum HealthStatus {
    #[default]
    Unknown,
    Healthy,
    Unhealthy(String),
    Checking,
}

#[derive(Clone)]
pub struct EndpointDisplay {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub protocol: String,
    pub tags: Vec<String>,
    pub enabled: bool,
    pub latency: Option<u64>, // Latency in milliseconds
    pub testing: bool, // Whether this endpoint is currently being tested
    pub test_error: Option<String>, // Error message if test failed
    pub test_diagnostics: Vec<String>, // Detailed diagnostic messages
    pub error_category: Option<String>, // Error category (DNS, Connection Refused, Timeout, etc.)
    pub is_connected: bool, // Whether this endpoint is currently connected
    pub raw_config: Option<serde_json::Value>, // Full config for protocols like VMess (UUID, etc.)
}

impl Default for EndpointDisplay {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            host: String::new(),
            port: 0,
            protocol: String::new(),
            tags: Vec::new(),
            enabled: false,
            latency: None,
            testing: false,
            test_error: None,
            test_diagnostics: Vec::new(),
            error_category: None,
            is_connected: false,
            raw_config: None,
        }
    }
}

#[derive(Clone)]
pub struct ConnectionInfo {
    pub endpoint_name: String,
    pub status: String,
    pub latency: Option<u64>, // ms
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub connected_at: String,
}

#[derive(Clone)]
pub struct SubscriptionInfo {
    pub name: String,
    pub url: String,
    pub last_update: String,
    pub endpoint_count: usize,
}

#[derive(Clone)]
pub struct XraySettings {
    pub log_level: String,
    pub dns_servers: Vec<String>,
    pub routing_domain_strategy: String,
    pub enable_sniffing: bool,
    pub enable_fragment: bool,
}

#[derive(Clone)]
pub struct ClientParams {
    pub system_proxy: bool,
    pub auto_start: bool,
    pub start_minimized: bool,
    pub allow_lan: bool,
    pub local_dns: bool,
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Configure fonts once (as fallback if egui-chinese-font didn't work)
        if !self.fonts_configured {
            Self::configure_fonts(ctx);
            self.fonts_configured = true;
        }
        
        // Apply cyberpunk theme
        self.theme.apply(ctx);

        // Top bar with title and status
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading(egui::RichText::new("SMART RELAY").size(28.0).strong().color(egui::Color32::from_rgb(0, 240, 255)));
                ui.label(egui::RichText::new("AI-Powered Proxy Control").size(12.0).color(egui::Color32::GRAY));
                
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Status indicator
                    match &self.health_status {
                        HealthStatus::Unknown => {
                            ui.label(egui::RichText::new("●").color(egui::Color32::GRAY).size(16.0));
                            ui.label("Not connected");
                        }
                        HealthStatus::Healthy => {
                            ui.label(egui::RichText::new("●").color(egui::Color32::from_rgb(0, 240, 255)).size(16.0));
                            ui.label(egui::RichText::new("ONLINE").color(egui::Color32::from_rgb(0, 240, 255)));
                        }
                        HealthStatus::Unhealthy(err) => {
                            ui.label(egui::RichText::new("●").color(egui::Color32::from_rgb(255, 107, 214)).size(16.0));
                            ui.label(egui::RichText::new(format!("ERROR: {}", err)).color(egui::Color32::from_rgb(255, 107, 214)));
                        }
                        HealthStatus::Checking => {
                            ui.label(egui::RichText::new("●").color(egui::Color32::YELLOW).size(16.0));
                            ui.label("Checking...");
                        }
                    }
                });
            });
        });

        // Sidebar with tabs
        egui::SidePanel::left("sidebar")
            .resizable(true)
            .default_width(200.0)
            .show(ctx, |ui| {
                ui.vertical(|ui| {
                    ui.add_space(10.0);
                    ui.selectable_value(&mut self.selected_tab, Tab::Overview, "📊 Overview");
                    ui.selectable_value(&mut self.selected_tab, Tab::Endpoints, "🔗 Endpoints");
                    ui.selectable_value(&mut self.selected_tab, Tab::Subscriptions, "📥 Subscriptions");
                    ui.selectable_value(&mut self.selected_tab, Tab::Configuration, "⚙️ Configuration");
                    ui.selectable_value(&mut self.selected_tab, Tab::Xray, "🚀 Xray Settings");
                    ui.selectable_value(&mut self.selected_tab, Tab::AI, "🤖 AI Assistant");
                    
                    ui.add_space(20.0);
                    ui.separator();
                    ui.add_space(10.0);
                    
                    let refresh_btn = ui.button(egui::RichText::new("🔄 Refresh All").color(egui::Color32::BLACK));
                    if refresh_btn.clicked() {
                        self.check_health();
                        self.load_endpoints();
                    }
                });
            });

        // Main content area
        egui::CentralPanel::default().show(ctx, |ui| {
            match self.selected_tab {
                Tab::Overview => self.show_overview(ui),
                Tab::Endpoints => self.show_endpoints(ui),
                Tab::Subscriptions => self.show_subscriptions(ui),
                Tab::Configuration => self.show_configuration(ui),
                Tab::Xray => self.show_xray_settings(ui),
                Tab::AI => self.show_ai_assistant(ui),
            }
        });

        // Process core download results
        if let Some(ref mut rx) = self.xray_download_receiver {
            if let Ok(result) = rx.try_recv() {
                self.downloading_xray = false;
                match result {
                    Ok(path) => {
                        self.xray_download_status = format!("✓ Downloaded to: {}", path.display());
                        self.xray_download_error = None;
                    }
                    Err(e) => {
                        self.xray_download_error = Some(e);
                        self.xray_download_status.clear();
                    }
                }
                self.xray_download_receiver = None;
            }
        }
        
        if let Some(ref mut rx) = self.sing_box_download_receiver {
            if let Ok(result) = rx.try_recv() {
                self.downloading_sing_box = false;
                match result {
                    Ok(path) => {
                        self.sing_box_download_status = format!("✓ Downloaded to: {}", path.display());
                        self.sing_box_download_error = None;
                    }
                    Err(e) => {
                        self.sing_box_download_error = Some(e);
                        self.sing_box_download_status.clear();
                    }
                }
                self.sing_box_download_receiver = None;
            }
        }
        
        // Process pending subscription results from receiver
        if let Some(ref mut rx) = self.subscription_receiver {
            if let Ok(result) = rx.try_recv() {
                match result {
                    Ok((sub_url, endpoints)) => {
                        self.subscription_status = format!("Parsing {} endpoints...", endpoints.len());
                        tracing::info!("Processing {} endpoints from subscription", endpoints.len());
                        
                        let sub_name = format!("Subscription {}", self.subscriptions.len() + 1);
                        self.subscriptions.push(SubscriptionInfo {
                            name: sub_name,
                            url: sub_url.clone(),
                            last_update: "Just now".to_string(),
                            endpoint_count: endpoints.len(),
                        });
                        
                        let mut added = 0;
                        for ep in endpoints {
                            self.endpoints.push(EndpointDisplay {
                                id: uuid::Uuid::new_v4().to_string(),
                                name: ep.name,
                                host: ep.host,
                                port: ep.port,
                                protocol: ep.protocol,
                                tags: ep.tags,
                                enabled: true,
                                latency: None,
                                testing: false,
                                test_error: None,
                                test_diagnostics: Vec::new(),
                                error_category: None,
                                is_connected: false,
                                raw_config: ep.raw_config.clone(),
                            });
                            added += 1;
                        }
                        
                        self.subscription_success = Some(format!("Successfully imported {} endpoints", added));
                        self.subscription_loading = false;
                        self.subscription_status.clear();
                        self.subscription_receiver = None;
                        tracing::info!("Subscription import completed: {} endpoints added", added);
                    }
                    Err(err) => {
                        tracing::error!("Subscription import failed: {}", err);
                        self.subscription_error = Some(err);
                        self.subscription_loading = false;
                        self.subscription_status.clear();
                        self.subscription_receiver = None;
                    }
                }
            } else if self.subscription_loading {
                if self.subscription_status.is_empty() || self.subscription_status == "Downloading subscription..." {
                    self.subscription_status = "Downloading...".to_string();
                }
            }
        }
        
        // Process test results from receiver
        if let Some(ref mut rx) = self.test_result_receiver {
            // Process all available test results (non-blocking)
            while let Ok((id, result)) = rx.try_recv() {
                // Find the endpoint by ID
                if let Some(ep) = self.endpoints.iter_mut().find(|e| e.id == id) {
                    // Mark as not testing
                    ep.testing = false;
                    
                    // Update based on result
                    if result.is_success() {
                        if let Some(latency) = result.latency {
                            ep.latency = Some(latency);
                            ep.test_error = None;
                            ep.error_category = None;
                            ep.test_diagnostics.clear();
                        }
                    } else {
                        // Test failed - set error information
                        ep.latency = None;
                        ep.test_error = result.error_message.clone();
                        ep.error_category = result.error_category.as_ref()
                            .map(|c| format!("{:?}", c));
                        ep.test_diagnostics = result.diagnostics.clone();
                    }
                    
                    // Remove from testing set
                    self.testing_endpoints.remove(&id);
                }
            }
        }

        ctx.request_repaint();
    }
}

impl App {
    pub fn set_runtime(&mut self, rt: Arc<Runtime>) {
        self.runtime = Some(rt);
        // Initialize xray adapter if xray is already downloaded
        if self.xray_adapter.is_none() {
            if let Ok(adapter) = XrayAdapter::new(self.socks_port, self.http_port, 10085) {
                self.xray_adapter = Some(Arc::new(adapter));
                tracing::info!("Xray adapter initialized");
            } else {
                tracing::debug!("Xray not available yet, will initialize when downloaded");
            }
        }
    }
    
    /// Convert EndpointDisplay to EndpointConfig for xray
    fn endpoint_to_config(&self, ep: &EndpointDisplay) -> anyhow::Result<EndpointConfig> {
        let mut config = EndpointConfig {
            host: ep.host.clone(),
            port: ep.port,
            protocol: ep.protocol.clone(),
            uuid: None,
            password: None,
            username: None,
            method: None,
            encryption: None,
            flow: None,
            alter_id: None,
            security: None,
            network: None,
            path: None,
            host_header: None,
            tls: None,
        };
        
        // Extract from raw_config if available (VMess, Shadowsocks, etc.)
        if let Some(ref raw) = ep.raw_config {
            if ep.protocol.to_lowercase() == "vmess" {
                config.uuid = raw.get("id").or_else(|| raw.get("uuid"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                config.alter_id = raw.get("aid").or_else(|| raw.get("alterId"))
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u16);
                config.security = raw.get("scy").or_else(|| raw.get("security"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                config.network = raw.get("net").or_else(|| raw.get("network"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                config.path = raw.get("path").and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                if let Some(host) = raw.get("host").and_then(|v| v.as_str()) {
                    config.host_header = Some(host.to_string());
                }
                if raw.get("tls").and_then(|v| v.as_str()) == Some("tls") {
                    config.tls = Some(crate::proxy::xray_adapter::TlsConfig {
                        server_name: raw.get("sni").or_else(|| raw.get("serverName"))
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                        // Default to true (skip certificate verification) like v2rayN
                        // This prevents "i/o timeout" errors caused by expired/invalid certificates
                        // Reference: v2rayN Settings -> Parameters -> "Skip certificate verification by default"
                        // Common issue: https://github.com/SagerNet/sing-box/issues/3001
                        allow_insecure: true,
                    });
                }
            } else if ep.protocol.to_lowercase() == "vless" {
                // Extract UUID and other VLESS settings from outbound config
                // raw_config is the full outbound JSON object: { "protocol": "vless", "settings": { "vnext": [...] } }
                if let Some(settings) = raw.get("settings") {
                    if let Some(vnext) = settings.get("vnext").and_then(|v| v.as_array()) {
                        if let Some(server) = vnext.first() {
                            if let Some(users) = server.get("users").and_then(|u| u.as_array()) {
                                if let Some(user) = users.first() {
                                    // Extract UUID (required for VLESS)
                                    config.uuid = user.get("id")
                                        .and_then(|v| v.as_str())
                                        .map(|s| s.to_string());
                                    
                                    if config.uuid.is_none() {
                                        tracing::warn!("VLESS: Could not extract UUID from raw_config. User object: {:?}", user);
                                    }
                                    
                                    // Extract encryption (defaults to "none")
                                    config.encryption = user.get("encryption")
                                        .and_then(|v| v.as_str())
                                        .map(|s| s.to_string());
                                    
                                    // Extract flow (for XTLS, e.g., "xtls-rprx-vision")
                                    config.flow = user.get("flow")
                                        .and_then(|v| v.as_str())
                                        .map(|s| s.to_string());
                                } else {
                                    tracing::warn!("VLESS: No user found in users array");
                                }
                            } else {
                                tracing::warn!("VLESS: No users array found in server");
                            }
                        } else {
                            tracing::warn!("VLESS: No server found in vnext array");
                        }
                    } else {
                        tracing::warn!("VLESS: No vnext array found in settings");
                    }
                } else {
                    tracing::warn!("VLESS: No settings found in raw_config. Raw config keys: {:?}", raw.as_object().map(|o| o.keys().collect::<Vec<_>>()));
                }
                
                // Extract stream settings
                if let Some(stream_settings) = raw.get("streamSettings") {
                    // Extract network type
                    config.network = stream_settings.get("network")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    
                    // Extract TLS settings
                    if stream_settings.get("security").and_then(|v| v.as_str()) == Some("tls") {
                        if let Some(tls_settings) = stream_settings.get("tlsSettings") {
                            config.tls = Some(crate::proxy::xray_adapter::TlsConfig {
                                server_name: tls_settings.get("serverName")
                                    .and_then(|v| v.as_str())
                                    .map(|s| s.to_string()),
                                allow_insecure: tls_settings.get("allowInsecure")
                                    .and_then(|v| v.as_bool())
                                    .unwrap_or(true), // Default to true like v2rayN
                            });
                        }
                    }
                    
                    // Extract path for WebSocket
                    if let Some(ws_settings) = stream_settings.get("wsSettings") {
                        config.path = ws_settings.get("path")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string());
                        if let Some(headers) = ws_settings.get("headers") {
                            if let Some(host) = headers.get("Host").and_then(|v| v.as_str()) {
                                config.host_header = Some(host.to_string());
                            }
                        }
                    }
                }
            } else if ep.protocol.to_lowercase() == "shadowsocks" {
                // Extract method and password for Shadowsocks
                config.method = raw.get("method")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                config.password = raw.get("password")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
            } else if ep.protocol.to_lowercase() == "trojan" {
                // Extract password for Trojan
                config.password = raw.get("password")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
            }
        }
        
        Ok(config)
    }
    
    fn show_overview(&mut self, ui: &mut egui::Ui) {
        ui.heading("Overview");
        ui.add_space(10.0);

        // Current Connection
        egui::Frame::group(ui.style())
            .fill(egui::Color32::from_rgb(11, 15, 26))
            .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(0, 240, 255)))
            .show(ui, |ui| {
                ui.heading(egui::RichText::new("Current Connection").color(egui::Color32::from_rgb(0, 240, 255)));
                ui.add_space(10.0);
                
                if let Some(conn) = &self.current_connection {
                    ui.label(format!("Endpoint: {}", conn.endpoint_name));
                    ui.label(format!("Status: {}", conn.status));
                    if let Some(latency) = conn.latency {
                        ui.label(format!("Latency: {} ms", latency));
                    }
                    ui.label(format!("Sent: {} bytes", conn.bytes_sent));
                    ui.label(format!("Received: {} bytes", conn.bytes_received));
                    ui.label(format!("Connected: {}", conn.connected_at));
                    
                    ui.add_space(10.0);
                    let disconnect_btn = ui.button(egui::RichText::new("Disconnect").color(egui::Color32::BLACK));
                    if disconnect_btn.clicked() {
                        self.current_connection = None;
                    }
                } else {
                    ui.label(egui::RichText::new("Not connected").italics().color(egui::Color32::GRAY));
                    let connect_btn = ui.button(egui::RichText::new("Connect").color(egui::Color32::BLACK));
                    if connect_btn.clicked() {
                        // Placeholder
                        self.current_connection = Some(ConnectionInfo {
                            endpoint_name: "Example Endpoint".to_string(),
                            status: "Connected".to_string(),
                            latency: Some(45),
                            bytes_sent: 1024,
                            bytes_received: 2048,
                            connected_at: "Just now".to_string(),
                        });
                    }
                }
            });

        ui.add_space(15.0);

        // Quick Stats
        egui::Frame::group(ui.style())
            .fill(egui::Color32::from_rgb(11, 15, 26))
            .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(255, 107, 214)))
            .show(ui, |ui| {
                ui.heading(egui::RichText::new("Quick Stats").color(egui::Color32::from_rgb(255, 107, 214)));
                ui.add_space(10.0);
                ui.label(format!("Total Endpoints: {}", self.endpoints.len()));
                ui.label(format!("Active Endpoints: {}", self.endpoints.iter().filter(|e| e.enabled).count()));
                ui.label(format!("Subscriptions: {}", self.subscriptions.len()));
            });
    }

    fn show_endpoints(&mut self, ui: &mut egui::Ui) {
        ui.heading("Proxy Endpoints");
        
        // Check Xray/proxy status
        ui.horizontal(|ui| {
            ui.label("Proxy Status: ");
            if let Some(ref rt) = self.runtime {
                let socks_port = self.socks_port;
                let http_port = self.http_port;
                let rt_clone = rt.clone();
                
                if ui.button(egui::RichText::new("🔍 Check").color(egui::Color32::BLACK)).clicked() {
                    rt_clone.spawn(async move {
                        let (socks_ok, http_ok) = xray_check::check_local_proxy_ports(socks_port, http_port).await;
                        if socks_ok || http_ok {
                            tracing::info!("Local proxy ports are listening: SOCKS={}, HTTP={}", socks_ok, http_ok);
                        } else {
                            tracing::warn!("Local proxy ports are not listening - Xray may not be running");
                        }
                    });
                }
            }
        });
        
        ui.add_space(10.0);

        ui.horizontal(|ui| {
            let add_btn = ui.button(egui::RichText::new("➕ Add Endpoint").color(egui::Color32::BLACK));
            if add_btn.clicked() {
                self.show_add_endpoint = true;
            }
            let refresh_btn = ui.button(egui::RichText::new("🔄 Refresh").color(egui::Color32::BLACK));
            if refresh_btn.clicked() {
                self.load_endpoints();
            }
            let batch_test_btn = ui.button(egui::RichText::new("🧪 Batch Test All").color(egui::Color32::BLACK));
            if batch_test_btn.clicked() {
                self.batch_test_endpoints();
            }
        });

        ui.add_space(10.0);

        if self.endpoints.is_empty() {
            ui.label(egui::RichText::new("No endpoints configured").italics().color(egui::Color32::GRAY));
        } else {
            egui::ScrollArea::vertical().show(ui, |ui| {
                egui::Grid::new("endpoints_grid")
                    .num_columns(7)
                    .spacing([10.0, 5.0])
                    .show(ui, |ui| {
                        ui.label(egui::RichText::new("ENABLED").strong());
                        ui.label(egui::RichText::new("NAME").strong());
                        ui.label(egui::RichText::new("HOST").strong());
                        ui.label(egui::RichText::new("PORT").strong());
                        ui.label(egui::RichText::new("PROTOCOL").strong());
                        ui.label(egui::RichText::new("ACTIONS").strong());
                        ui.end_row();

                        let mut to_remove = None;
                        let mut to_edit = None;
                        let mut to_test = None;
                        let mut to_connect = None;
                        let mut to_disconnect = None;
                        
                        for (idx, ep) in self.endpoints.iter_mut().enumerate() {
                            let row_response = ui.allocate_response(egui::Vec2::new(ui.available_width(), 0.0), egui::Sense::click());
                            
                            // Right-click context menu
                            if row_response.secondary_clicked() {
                                self.right_click_menu = Some((row_response.interact_pointer_pos().unwrap_or(egui::Pos2::ZERO), idx));
                            }
                            
                            // Highlight connected endpoint
                            if ep.is_connected {
                                ui.painter().rect_filled(
                                    row_response.rect,
                                    0.0,
                                    egui::Color32::from_rgb(0, 240, 255).linear_multiply(0.1),
                                );
                            }
                            
                            ui.checkbox(&mut ep.enabled, "");
                            
                            // Name with connection indicator
                            let name_text = if ep.is_connected {
                                format!("🔗 {}", ep.name)
                            } else {
                                ep.name.clone()
                            };
                            ui.label(egui::RichText::new(&name_text).color(if ep.is_connected {
                                egui::Color32::from_rgb(0, 240, 255)
                            } else {
                                egui::Color32::WHITE
                            }));
                            
                            ui.label(egui::RichText::new(&ep.host).monospace());
                            ui.label(ep.port.to_string());
                            ui.label(&ep.protocol);
                            
                            // Latency display with error category
                            if let Some(latency) = ep.latency {
                                let latency_color = if latency < 100 {
                                    egui::Color32::from_rgb(0, 255, 0)
                                } else if latency < 300 {
                                    egui::Color32::from_rgb(255, 255, 0)
                                } else {
                                    egui::Color32::from_rgb(255, 100, 100)
                                };
                                ui.label(egui::RichText::new(format!("{}ms", latency)).color(latency_color));
                            } else if ep.testing {
                                ui.label(egui::RichText::new("Testing...").color(egui::Color32::YELLOW));
                            } else if let Some(ref category) = ep.error_category {
                                // Show error category icon with tooltip
                                let (icon, color) = match category.as_str() {
                                    "DnsFailure" => ("🌐", egui::Color32::from_rgb(255, 150, 0)),
                                    "ConnectionRefused" => ("🚫", egui::Color32::from_rgb(255, 107, 214)),
                                    "ConnectionTimeout" => ("⏱️", egui::Color32::from_rgb(255, 200, 0)),
                                    "NetworkUnreachable" => ("📡", egui::Color32::from_rgb(255, 100, 100)),
                                    _ => ("❌", egui::Color32::from_rgb(255, 107, 214)),
                                };
                                let icon_label = ui.label(egui::RichText::new(icon).color(color));
                                if icon_label.hovered() && !ep.test_diagnostics.is_empty() {
                                    egui::show_tooltip_at_pointer(ui.ctx(), egui::Id::new("endpoint_diag"), |ui: &mut egui::Ui| {
                                        ui.set_max_width(400.0);
                                        ui.label(egui::RichText::new("Click for details").strong());
                                        ui.separator();
                                        for diag in &ep.test_diagnostics {
                                            ui.label(egui::RichText::new(diag).small());
                                        }
                                    });
                                }
                                if icon_label.clicked() {
                                    self.show_diagnostics = Some(idx);
                                }
                            } else if let Some(ref _err) = ep.test_error {
                                ui.label(egui::RichText::new("❌").color(egui::Color32::from_rgb(255, 107, 214)));
                            } else {
                                ui.label(egui::RichText::new("-").color(egui::Color32::GRAY));
                            }
                            
                            ui.horizontal(|ui| {
                                if ep.is_connected {
                                    if ui.small_button(egui::RichText::new("Disconnect").color(egui::Color32::BLACK)).clicked() {
                                        to_disconnect = Some(idx);
                                    }
                                } else {
                                    if ui.small_button(egui::RichText::new("Connect").color(egui::Color32::BLACK)).clicked() {
                                        to_connect = Some(idx);
                                    }
                                }
                                
                                if ui.small_button(egui::RichText::new(if ep.testing { "⏳" } else { "Test" }).color(egui::Color32::BLACK)).clicked() {
                                    to_test = Some(idx);
                                }
                                
                                if ui.small_button(egui::RichText::new("Edit").color(egui::Color32::BLACK)).clicked() {
                                    to_edit = Some(idx);
                                }
                                if ui.small_button(egui::RichText::new("Delete").color(egui::Color32::BLACK)).clicked() {
                                    to_remove = Some(idx);
                                }
                            });
                            ui.end_row();
                        }
                        
                        // Handle actions
                        if let Some(idx) = to_test {
                            self.test_endpoint(idx);
                        }
                        if let Some(idx) = to_connect {
                            self.connect_endpoint(idx);
                        }
                        if let Some(_idx) = to_disconnect {
                            self.disconnect_endpoint();
                        }
                        
                        if let Some(idx) = to_remove {
                            self.endpoints.remove(idx);
                        }
                        if let Some(idx) = to_edit {
                            self.editing_endpoint = Some(idx);
                        }
                    });
            });
        }

        // Add/Edit endpoint dialog
        if self.show_add_endpoint || self.editing_endpoint.is_some() {
            self.show_endpoint_dialog(ui);
        }
    }

    fn show_endpoint_dialog(&mut self, ui: &mut egui::Ui) {
        let is_editing = self.editing_endpoint.is_some();
        let window_title = if is_editing { "Edit Endpoint" } else { "Add Endpoint" };
        
        egui::Window::new(window_title)
            .collapsible(false)
            .resizable(true)
            .show(ui.ctx(), |ui| {
                // Get existing endpoint data if editing
                let mut name = String::new();
                let mut host = String::new();
                let mut port_str = String::new();
                let mut protocol = "vmess".to_string();
                
                if let Some(idx) = self.editing_endpoint {
                    if let Some(ep) = self.endpoints.get(idx) {
                        name = ep.name.clone();
                        host = ep.host.clone();
                        port_str = ep.port.to_string();
                        protocol = ep.protocol.clone();
                    }
                }
                
                ui.label("Name:");
                ui.text_edit_singleline(&mut name);
                
                ui.label("Host:");
                ui.text_edit_singleline(&mut host);
                
                ui.label("Port:");
                ui.text_edit_singleline(&mut port_str);
                
                ui.label("Protocol:");
                egui::ComboBox::from_label("")
                    .selected_text(&protocol)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut protocol, "vmess".to_string(), "VMess");
                        ui.selectable_value(&mut protocol, "vless".to_string(), "VLESS");
                        ui.selectable_value(&mut protocol, "shadowsocks".to_string(), "Shadowsocks");
                        ui.selectable_value(&mut protocol, "trojan".to_string(), "Trojan");
                        ui.selectable_value(&mut protocol, "socks5".to_string(), "SOCKS5");
                    });
                
                ui.horizontal(|ui| {
                    let save_btn = ui.button(egui::RichText::new(if is_editing { "Update" } else { "Save" }).color(egui::Color32::BLACK));
                    if save_btn.clicked() {
                        if let Ok(port) = port_str.parse::<u16>() {
                            if let Some(idx) = self.editing_endpoint {
                                // Update existing endpoint
                                if let Some(ep) = self.endpoints.get_mut(idx) {
                                    ep.name = name;
                                    ep.host = host;
                                    ep.port = port;
                                    ep.protocol = protocol;
                                }
                                self.editing_endpoint = None;
                            } else {
                                // Add new endpoint
                                self.endpoints.push(EndpointDisplay {
                                    id: uuid::Uuid::new_v4().to_string(),
                                    name,
                                    host,
                                    port,
                                    protocol,
                                    tags: vec![],
                                    enabled: true,
                                    latency: None,
                                    testing: false,
                                    test_error: None,
                                    test_diagnostics: Vec::new(),
                                    error_category: None,
                                    is_connected: false,
                                    raw_config: None,
                                });
                            }
                            self.show_add_endpoint = false;
                        }
                    }
                    let cancel_btn = ui.button(egui::RichText::new("Cancel").color(egui::Color32::BLACK));
                    if cancel_btn.clicked() {
                        self.show_add_endpoint = false;
                        self.editing_endpoint = None;
                    }
                });
            });
    }

    fn show_subscriptions(&mut self, ui: &mut egui::Ui) {
        ui.heading("Subscriptions");
        ui.add_space(10.0);

        // Import subscription
        egui::Frame::group(ui.style())
            .fill(egui::Color32::from_rgb(11, 15, 26))
            .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(0, 240, 255)))
            .show(ui, |ui| {
                ui.heading(egui::RichText::new("Import Subscription").color(egui::Color32::from_rgb(0, 240, 255)));
                ui.add_space(10.0);
                
                ui.label("Subscription URL:");
                ui.text_edit_singleline(&mut self.subscription_url);
                
                ui.horizontal(|ui| {
                    let import_btn = ui.button(egui::RichText::new("📥 Import from URL").color(egui::Color32::BLACK));
                    if import_btn.clicked() {
                        self.import_subscription_url();
                    }
                    let file_btn = ui.button(egui::RichText::new("📁 Import from File").color(egui::Color32::BLACK));
                    if file_btn.clicked() {
                        // File picker would go here
                    }
                });
                
                if self.subscription_loading {
                    ui.spinner();
                    if !self.subscription_status.is_empty() {
                        ui.label(egui::RichText::new(&self.subscription_status).color(egui::Color32::from_rgb(0, 240, 255)));
                    } else {
                        ui.label("Downloading subscription...");
                    }
                }
                
                if let Some(err) = &self.subscription_error {
                    ui.label(egui::RichText::new(format!("Error: {}", err)).color(egui::Color32::from_rgb(255, 107, 214)));
                }
                
                if let Some(success) = &self.subscription_success {
                    ui.label(egui::RichText::new(success).color(egui::Color32::from_rgb(0, 240, 255)));
                }
            });

        ui.add_space(15.0);

        // Subscription list
        if self.subscriptions.is_empty() {
            ui.label(egui::RichText::new("No subscriptions").italics().color(egui::Color32::GRAY));
        } else {
            egui::ScrollArea::vertical().show(ui, |ui| {
                        let mut to_update = None;
                        let mut to_delete = None;
                        
                        for sub in &self.subscriptions {
                            egui::Frame::group(ui.style())
                                .fill(egui::Color32::from_rgb(11, 15, 26))
                                .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(255, 107, 214)))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.vertical(|ui| {
                                            ui.label(egui::RichText::new(&sub.name).strong());
                                            ui.label(egui::RichText::new(&sub.url).monospace().small().color(egui::Color32::GRAY));
                                            ui.label(format!("Endpoints: {} | Last update: {}", sub.endpoint_count, sub.last_update));
                                        });
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            let update_btn = ui.button(egui::RichText::new("🔄 Update").color(egui::Color32::BLACK));
                                            if update_btn.clicked() {
                                                to_update = Some(sub.url.clone());
                                            }
                                            let delete_btn = ui.button(egui::RichText::new("🗑️ Delete").color(egui::Color32::BLACK));
                                            if delete_btn.clicked() {
                                                to_delete = Some(sub.url.clone());
                                            }
                                        });
                                    });
                                });
                            ui.add_space(5.0);
                        }
                        
                        if let Some(url) = to_update {
                            self.update_subscription(&url);
                        }
                        if let Some(url) = to_delete {
                            self.delete_subscription(&url);
                        }
            });
        }
    }

    fn show_configuration(&mut self, ui: &mut egui::Ui) {
        ui.heading("Client Configuration");
        ui.add_space(10.0);

        egui::ScrollArea::vertical().show(ui, |ui| {
            // Port Configuration
            egui::Frame::group(ui.style())
                .fill(egui::Color32::from_rgb(11, 15, 26))
                .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(0, 240, 255)))
                .show(ui, |ui| {
                    ui.heading(egui::RichText::new("Port Configuration").color(egui::Color32::from_rgb(0, 240, 255)));
                    ui.add_space(10.0);
                    
                    ui.horizontal(|ui| {
                        ui.label("Local Port:");
                        let mut port = self.local_port as f64;
                        if ui.add(egui::DragValue::new(&mut port).speed(1.0)).changed() {
                            self.local_port = port.clamp(1024.0, 65535.0) as u16;
                        }
                    });
                    
                    ui.horizontal(|ui| {
                        ui.label("SOCKS5 Port:");
                        let mut port = self.socks_port as f64;
                        if ui.add(egui::DragValue::new(&mut port).speed(1.0)).changed() {
                            self.socks_port = port.clamp(1024.0, 65535.0) as u16;
                        }
                    });
                    
                    ui.horizontal(|ui| {
                        ui.label("HTTP Port:");
                        let mut port = self.http_port as f64;
                        if ui.add(egui::DragValue::new(&mut port).speed(1.0)).changed() {
                            self.http_port = port.clamp(1024.0, 65535.0) as u16;
                        }
                    });
                });

            ui.add_space(15.0);

            // Client Parameters
            egui::Frame::group(ui.style())
                .fill(egui::Color32::from_rgb(11, 15, 26))
                .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(255, 107, 214)))
                .show(ui, |ui| {
                    ui.heading(egui::RichText::new("Client Parameters").color(egui::Color32::from_rgb(255, 107, 214)));
                    ui.add_space(10.0);
                    
                    ui.checkbox(&mut self.client_params.auto_start, "Auto Start on Boot");
                    ui.checkbox(&mut self.client_params.start_minimized, "Start Minimized");
                    ui.checkbox(&mut self.client_params.allow_lan, "Allow LAN Connections");
                    ui.checkbox(&mut self.client_params.local_dns, "Use Local DNS");
                    
                    ui.separator();
                    ui.add_space(10.0);
                    
                    ui.heading(egui::RichText::new("System Proxy Mode").color(egui::Color32::from_rgb(0, 240, 255)));
                    ui.add_space(5.0);
                    
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut self.proxy_mode, ProxyMode::Unchanged, "Unchanged");
                        ui.radio_value(&mut self.proxy_mode, ProxyMode::Direct, "Direct (No Proxy)");
                        ui.radio_value(&mut self.proxy_mode, ProxyMode::Global, "Global Proxy");
                        ui.radio_value(&mut self.proxy_mode, ProxyMode::Pac, "PAC Mode");
                    });
                    
                    if self.proxy_mode == ProxyMode::Global || self.proxy_mode == ProxyMode::Pac {
                        ui.add_space(5.0);
                        ui.label("Proxy Exceptions (semicolon-separated):");
                        ui.text_edit_singleline(&mut self.proxy_exceptions);
                        ui.checkbox(&mut self.client_params.system_proxy, "Apply to System");
                        
                        if ui.button(egui::RichText::new("Apply Proxy Settings").color(egui::Color32::BLACK)).clicked() {
                            self.apply_system_proxy();
                        }
                    }
                });
        });
    }

    fn show_xray_settings(&mut self, ui: &mut egui::Ui) {
        ui.heading("Xray Core Settings");
        ui.add_space(10.0);

        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::Frame::group(ui.style())
                .fill(egui::Color32::from_rgb(11, 15, 26))
                .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(0, 240, 255)))
                .show(ui, |ui| {
                    ui.heading(egui::RichText::new("Xray Configuration").color(egui::Color32::from_rgb(0, 240, 255)));
                    ui.add_space(10.0);
                    
                    ui.horizontal(|ui| {
                        ui.label("Log Level:");
                        egui::ComboBox::from_label("")
                            .selected_text(&self.xray_settings.log_level)
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.xray_settings.log_level, "debug".to_string(), "Debug");
                                ui.selectable_value(&mut self.xray_settings.log_level, "info".to_string(), "Info");
                                ui.selectable_value(&mut self.xray_settings.log_level, "warning".to_string(), "Warning");
                                ui.selectable_value(&mut self.xray_settings.log_level, "error".to_string(), "Error");
                            });
                    });
                    
                    ui.separator();
                    ui.add_space(5.0);
                    
                    ui.label("DNS Servers (one per line):");
                    let mut dns_text = self.xray_settings.dns_servers.join("\n");
                    ui.text_edit_multiline(&mut dns_text);
                    self.xray_settings.dns_servers = dns_text.lines().map(|s| s.to_string()).collect();
                    
                    ui.separator();
                    ui.add_space(10.0);
                    
                    // Core Download Section
                    ui.heading(egui::RichText::new("Core Downloads").color(egui::Color32::from_rgb(0, 240, 255)));
                    ui.add_space(5.0);
                    
                    // Xray download
                    ui.horizontal(|ui| {
                        let xray_exists = core_download::check_core_exists("xray");
                        if xray_exists {
                            ui.label(egui::RichText::new("✓ Xray").color(egui::Color32::from_rgb(0, 255, 0)));
                            if let Some(path) = core_download::get_core_path("xray") {
                                ui.label(egui::RichText::new(format!("({})", path.display())).small().color(egui::Color32::GRAY));
                            }
                        } else {
                            ui.label(egui::RichText::new("✗ Xray not installed").color(egui::Color32::from_rgb(255, 107, 214)));
                        }
                        
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let download_btn = ui.button(egui::RichText::new(if self.downloading_xray { "⏳ Downloading..." } else { "📥 Download Xray" }).color(egui::Color32::BLACK));
                            if download_btn.clicked() && !self.downloading_xray {
                                self.download_xray();
                            }
                        });
                    });
                    
                    if self.downloading_xray {
                        if !self.xray_download_status.is_empty() {
                            ui.label(egui::RichText::new(&self.xray_download_status).color(egui::Color32::from_rgb(0, 240, 255)));
                        }
                        ui.spinner();
                    }
                    
                    if let Some(err) = &self.xray_download_error {
                        ui.label(egui::RichText::new(format!("Error: {}", err)).color(egui::Color32::from_rgb(255, 107, 214)));
                    }
                    
                    ui.add_space(5.0);
                    
                    // Sing-box download
                    ui.horizontal(|ui| {
                        let sing_box_exists = core_download::check_core_exists("sing-box");
                        if sing_box_exists {
                            ui.label(egui::RichText::new("✓ sing-box").color(egui::Color32::from_rgb(0, 255, 0)));
                            if let Some(path) = core_download::get_core_path("sing-box") {
                                ui.label(egui::RichText::new(format!("({})", path.display())).small().color(egui::Color32::GRAY));
                            }
                        } else {
                            ui.label(egui::RichText::new("✗ sing-box not installed").color(egui::Color32::from_rgb(255, 107, 214)));
                        }
                        
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let download_btn = ui.button(egui::RichText::new(if self.downloading_sing_box { "⏳ Downloading..." } else { "📥 Download sing-box" }).color(egui::Color32::BLACK));
                            if download_btn.clicked() && !self.downloading_sing_box {
                                self.download_sing_box();
                            }
                        });
                    });
                    
                    if self.downloading_sing_box {
                        if !self.sing_box_download_status.is_empty() {
                            ui.label(egui::RichText::new(&self.sing_box_download_status).color(egui::Color32::from_rgb(0, 240, 255)));
                        }
                        ui.spinner();
                    }
                    
                    if let Some(err) = &self.sing_box_download_error {
                        ui.label(egui::RichText::new(format!("Error: {}", err)).color(egui::Color32::from_rgb(255, 107, 214)));
                    }
                    
                    ui.add_space(10.0);
                    ui.separator();
                    ui.add_space(10.0);
                    
                    // Test Configuration Section
                    ui.heading(egui::RichText::new("Test Configuration").color(egui::Color32::from_rgb(0, 240, 255)));
                    ui.add_space(5.0);
                    ui.label(egui::RichText::new("Load a working test configuration (tested with v2rayN)").small().color(egui::Color32::GRAY));
                    ui.add_space(5.0);
                    
                    if ui.button(egui::RichText::new("📋 Load Test Config").color(egui::Color32::BLACK)).clicked() {
                        self.load_test_config();
                    }
                    
                    ui.separator();
                    ui.add_space(5.0);
                    
                    ui.horizontal(|ui| {
                        ui.label("Routing Domain Strategy:");
                        egui::ComboBox::from_label("")
                            .selected_text(&self.xray_settings.routing_domain_strategy)
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.xray_settings.routing_domain_strategy, "AsIs".to_string(), "AsIs");
                                ui.selectable_value(&mut self.xray_settings.routing_domain_strategy, "IPIfNonMatch".to_string(), "IPIfNonMatch");
                                ui.selectable_value(&mut self.xray_settings.routing_domain_strategy, "IPOnDemand".to_string(), "IPOnDemand");
                            });
                    });
                    
                    ui.separator();
                    ui.add_space(5.0);
                    
                    ui.checkbox(&mut self.xray_settings.enable_sniffing, "Enable Traffic Sniffing");
                    ui.checkbox(&mut self.xray_settings.enable_fragment, "Enable Packet Fragmentation");
                });
        });
    }

    fn show_ai_assistant(&mut self, ui: &mut egui::Ui) {
        ui.heading("AI Assistant");
        ui.add_space(10.0);

        egui::Frame::group(ui.style())
            .fill(egui::Color32::from_rgb(11, 15, 26))
            .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(0, 240, 255)))
            .show(ui, |ui| {
                ui.heading(egui::RichText::new("AI-Powered Configuration").color(egui::Color32::from_rgb(0, 240, 255)));
                ui.add_space(10.0);

                ui.label("Describe your networking goal:");
                ui.text_edit_multiline(&mut self.ai_goal);
                ui.add_space(5.0);

                ui.horizontal(|ui| {
                    let button = ui.button(egui::RichText::new("🤖 Ask AI").strong().color(egui::Color32::BLACK));
                    if button.clicked() {
                        self.ask_ai();
                    }
                    if self.ai_loading {
                        ui.spinner();
                        ui.label("Thinking...");
                    }
                });

                if let Some(response) = &self.ai_response {
                    ui.add_space(10.0);
                    ui.separator();
                    ui.add_space(5.0);
                    ui.label(egui::RichText::new("AI Recommendation:").strong());
                    ui.add_space(5.0);
                    egui::Frame::group(ui.style())
                        .fill(egui::Color32::from_rgb(20, 25, 40))
                        .show(ui, |ui| {
                            ui.label(egui::RichText::new(response).monospace());
                        });
                    ui.add_space(10.0);
                    let apply_btn = ui.button(egui::RichText::new("Apply Configuration").color(egui::Color32::BLACK));
                    if apply_btn.clicked() {
                        // Apply AI recommendations
                    }
                }
            });
    }

    fn check_health(&mut self) {
        self.health_status = HealthStatus::Checking;
        // TODO: Async HTTP call
    }

    fn ask_ai(&mut self) {
        self.ai_loading = true;
        self.ai_response = None;
        // TODO: Async HTTP call to /ai/propose
        self.ai_response = Some(format!("AI recommendation for: {}\n\n[Connect to control plane at http://127.0.0.1:8080 to use AI features]", self.ai_goal));
        self.ai_loading = false;
    }

    fn load_endpoints(&mut self) {
        // TODO: Fetch from /config endpoint
        self.endpoints = vec![];
    }

    fn import_subscription_url(&mut self) {
        if self.subscription_url.is_empty() {
            return;
        }
        
        let Some(rt) = &self.runtime else {
            self.subscription_error = Some("Runtime not initialized".to_string());
            return;
        };
        
        let url = self.subscription_url.clone();
        self.subscription_url.clear();
        self.subscription_loading = true;
        self.subscription_error = None;
        self.subscription_success = None;
        self.subscription_status = format!("Connecting to {}...", url);
        
        // Spawn async task using the runtime
        let url_clone = url.clone();
        let (tx, rx) = mpsc::channel();
        
        tracing::info!("Spawning subscription download task for: {}", url_clone);
        
        // Check if we should use proxy (if Xray is running) and get the SOCKS port
        let (use_proxy, socks_port) = if let Some(ref adapter) = self.xray_adapter {
            if adapter.is_running() {
                (true, Some(adapter.get_actual_socks_port()))
            } else {
                (false, None)
            }
        } else {
            (false, None)
        };
        
        rt.spawn(async move {
            tracing::info!("Subscription download task started (proxy: {}, port: {:?})", use_proxy, socks_port);
            let result = match subscription::download_subscription(&url_clone, use_proxy, socks_port).await {
                Ok(content) => {
                    tracing::info!("Download successful, starting parse...");
                    subscription::parse_subscription_content(&content)
                        .map(|endpoints| {
                            tracing::info!("Parse successful, found {} endpoints", endpoints.len());
                            (url_clone, endpoints)
                        })
                        .map_err(|e| {
                            tracing::error!("Parse failed: {}", e);
                            format!("Failed to parse: {}", e)
                        })
                }
                Err(e) => {
                    tracing::error!("Download failed: {}", e);
                    Err(format!("Failed to download: {}", e))
                }
            };
            tracing::info!("Sending result to channel...");
            let _ = tx.send(result);
            tracing::info!("Result sent to channel");
        });
        
        // Store receiver for checking in update loop
        self.subscription_receiver = Some(rx);
        tracing::info!("Subscription receiver stored, waiting for results...");
    }
    
    fn update_subscription(&mut self, url: &str) {
        let Some(rt) = &self.runtime else {
            self.subscription_error = Some("Runtime not initialized".to_string());
            return;
        };
        
        self.subscription_loading = true;
        self.subscription_error = None;
        
        let url = url.to_string();
        
        // Check if we should use proxy (if Xray is running) and get the SOCKS port
        let (use_proxy, socks_port) = if let Some(ref adapter) = self.xray_adapter {
            if adapter.is_running() {
                (true, Some(adapter.get_actual_socks_port()))
            } else {
                (false, None)
            }
        } else {
            (false, None)
        };
        
        let (tx, rx) = mpsc::channel();
        rt.spawn(async move {
            let result = match subscription::download_subscription(&url, use_proxy, socks_port).await {
                Ok(content) => {
                    subscription::parse_subscription_content(&content)
                        .map(|endpoints| (url.clone(), endpoints))
                        .map_err(|e| format!("Failed to parse: {}", e))
                }
                Err(e) => Err(format!("Failed to download: {}", e)),
            };
            let _ = tx.send(result);
        });
        
        // Store receiver for checking in update loop
        self.subscription_receiver = Some(rx);
    }
    
    fn delete_subscription(&mut self, url: &str) {
        self.subscriptions.retain(|s| s.url != url);
    }
    
    fn download_xray(&mut self) {
        let Some(rt) = &self.runtime else {
            self.xray_download_error = Some("Runtime not initialized".to_string());
            return;
        };
        
        self.downloading_xray = true;
        self.xray_download_status = "Fetching latest version...".to_string();
        self.xray_download_error = None;
        
        let (tx, rx) = mpsc::channel();
        self.xray_download_receiver = Some(rx);
        
        rt.spawn(async move {
            match core_download::download_xray(None).await {
                Ok(path) => {
                    let _ = tx.send(Ok(path));
                }
                Err(e) => {
                    let _ = tx.send(Err(e.to_string()));
                }
            }
        });
    }
    
    fn download_sing_box(&mut self) {
        let Some(rt) = &self.runtime else {
            self.sing_box_download_error = Some("Runtime not initialized".to_string());
            return;
        };
        
        self.downloading_sing_box = true;
        self.sing_box_download_status = "Fetching latest version...".to_string();
        self.sing_box_download_error = None;
        
        let (tx, rx) = mpsc::channel();
        rt.spawn(async move {
            match core_download::download_sing_box(None).await {
                Ok(path) => {
                    let _ = tx.send(Ok(path));
                }
                Err(e) => {
                    let _ = tx.send(Err(e.to_string()));
                }
            }
        });
        
        self.sing_box_download_receiver = Some(rx);
    }
    
    fn test_endpoint(&mut self, idx: usize) {
        if idx >= self.endpoints.len() {
            return;
        }
        
        // Get endpoint data first
        let id = self.endpoints[idx].id.clone();
        let ep = self.endpoints[idx].clone();
        
        // Mark as testing
        self.endpoints[idx].testing = true;
        self.endpoints[idx].test_error = None;
        self.endpoints[idx].test_diagnostics.clear();
        self.testing_endpoints.insert(id.clone());
        
        // Get runtime
        let rt = match &self.runtime {
            Some(r) => r.clone(),
            None => {
                tracing::warn!("No runtime available for endpoint testing");
                self.endpoints[idx].testing = false;
                self.endpoints[idx].test_error = Some("No runtime available".to_string());
                return;
            }
        };
        
        // Create channel for test results (used by both Xray and direct testing)
        let (tx, rx) = mpsc::channel();
        
        // Store receiver if this is the first test
        if self.test_result_receiver.is_none() {
            self.test_result_receiver = Some(rx);
        } else {
            // For multiple concurrent tests, we'd need a different approach
            // For now, replace the receiver (in production, use a multi-receiver pattern)
            let _ = self.test_result_receiver.replace(rx);
        }
        
        // Try to use xray if available, otherwise fall back to direct testing
        if let Some(ref adapter) = self.xray_adapter {
            tracing::info!("Using Xray adapter for testing endpoint {}:{}", ep.host, ep.port);
            // Convert endpoint to config before async closure
            let endpoint_config = match self.endpoint_to_config(&ep) {
                Ok(config) => config,
                Err(e) => {
                    tracing::error!("Failed to convert endpoint to config: {}", e);
                    self.endpoints[idx].testing = false;
                    self.endpoints[idx].test_error = Some(format!("Config error: {}", e));
                    self.endpoints[idx].error_category = Some("ConfigurationError".to_string());
                    return;
                }
            };
            
            // Use xray for testing - this actually starts xray and tests through it
            let adapter_clone = adapter.clone();
            let id_clone = id.clone();
            let tx_clone = tx.clone();
            
            rt.spawn(async move {
                // Test using xray
                match adapter_clone.test_endpoint(&endpoint_config, Duration::from_secs(10)).await {
                    Ok(probe_result) => {
                        tracing::info!("Xray test successful: {}ms", probe_result.latency_ms);
                        // Convert to TestResult and send through channel
                        let result = endpoint_test::TestResult::success(probe_result.latency_ms as u64);
                        let _ = tx_clone.send((id_clone, result));
                    }
                    Err(e) => {
                        tracing::error!("Xray test failed: {}", e);
                        // Determine error category from error message
                        let error_msg = e.to_string();
                        let category = if error_msg.contains("timeout") || error_msg.contains("Timeout") {
                            endpoint_test::ErrorCategory::ConnectionTimeout
                        } else if error_msg.contains("unreachable") || error_msg.contains("Unreachable") {
                            endpoint_test::ErrorCategory::NetworkUnreachable
                        } else if error_msg.contains("refused") || error_msg.contains("Refused") {
                            endpoint_test::ErrorCategory::ConnectionRefused
                        } else {
                            endpoint_test::ErrorCategory::Unknown
                        };
                        let result = endpoint_test::TestResult::failure(
                            category,
                            error_msg.clone(),
                            vec![format!("Xray proxy test failed: {}", error_msg)]
                        );
                        let _ = tx_clone.send((id_clone, result));
                    }
                }
            });
        } else {
            tracing::info!("Xray adapter not available, using direct testing for endpoint {}:{}", ep.host, ep.port);
            // Fallback to direct testing (existing implementation)
            let host = ep.host.clone();
            let port = ep.port;
            let protocol = ep.protocol.clone();
            let host_clone = host.clone();
            let id_clone = id.clone();
            let protocol_clone = protocol.clone();
            let socks_port = self.socks_port;
            
            // Spawn comprehensive async test task with smart testing
            rt.spawn(async move {
                let result = endpoint_test::test_endpoint_smart(
                    &host_clone, 
                    port, 
                    &protocol_clone,
                    socks_port,
                ).await;
                
                // Send result back to GUI
                let _ = tx.send((id_clone.clone(), result.clone()));
                
                // Log detailed results
                if result.is_success() {
                    tracing::info!("Test result for {}: Success ({}ms)", id_clone, result.latency.unwrap_or(0));
                } else {
                    let category = result.error_category.as_ref()
                        .map(|c| format!("{:?}", c))
                        .unwrap_or_else(|| "Unknown".to_string());
                    tracing::info!("Test result for {}: Failed ({}) - {}", 
                        id_clone, category, 
                        result.error_message.as_ref().unwrap_or(&"Unknown error".to_string()));
                    for diag in &result.diagnostics {
                        tracing::debug!("  Diagnostic: {}", diag);
                    }
                }
            });
        }
    }
    
    fn batch_test_endpoints(&mut self) {
        tracing::info!("=== Starting batch test for all enabled endpoints ===");
        
        // Collect endpoint data separately to avoid borrow issues
        let endpoint_data: Vec<(String, String, u16, String, u16)> = self.endpoints
            .iter()
            .filter(|ep| ep.enabled)
            .map(|ep| (ep.id.clone(), ep.host.clone(), ep.port, ep.protocol.clone(), self.socks_port))
            .collect();
        
        if endpoint_data.is_empty() {
            tracing::warn!("No enabled endpoints to test");
            return;
        }
        
        tracing::info!("Found {} enabled endpoints to test", endpoint_data.len());
        
        // Mark all as testing (separate from data collection)
        let endpoint_ids: Vec<String> = self.endpoints
            .iter()
            .filter(|ep| ep.enabled)
            .map(|ep| ep.id.clone())
            .collect();
        
        let mut count = 0;
        for ep in &mut self.endpoints {
            if endpoint_ids.contains(&ep.id) {
                ep.testing = true;
                ep.test_error = None;
                ep.test_diagnostics.clear();
                ep.error_category = None;
                self.testing_endpoints.insert(ep.id.clone());
                count += 1;
            }
        }
        
        tracing::info!("Marked {} endpoints as testing", count);
        
        let rt = match &self.runtime {
            Some(r) => r.clone(),
            None => {
                tracing::error!("No runtime available for batch testing");
                // Reset testing state
                for ep in &mut self.endpoints {
                    if endpoint_ids.contains(&ep.id) {
                        ep.testing = false;
                        ep.test_error = Some("No runtime available".to_string());
                        self.testing_endpoints.remove(&ep.id);
                    }
                }
                return;
            }
        };
        
        // Create channel for batch test results
        let (tx, rx) = mpsc::channel();
        
        // Store receiver for processing results
        if self.test_result_receiver.is_none() {
            self.test_result_receiver = Some(rx);
        } else {
            let _ = self.test_result_receiver.replace(rx);
        }
        
        tracing::info!("Spawning batch test task for {} endpoints...", endpoint_data.len());
        
        // Spawn batch test with comprehensive diagnostics
        rt.spawn(async move {
            tracing::info!("Batch test task started, testing {} endpoints...", endpoint_data.len());
            let start_time = std::time::Instant::now();
            
            let results = endpoint_test::batch_test_endpoints(endpoint_data).await;
            
            let elapsed = start_time.elapsed();
            tracing::info!("Batch test completed in {:.2}s: {} results received", elapsed.as_secs_f64(), results.len());
            
            let mut success_count = 0;
            let mut failure_count = 0;
            
            // Send all results back to GUI
            for (id, result) in results.iter() {
                let _ = tx.send((id.clone(), result.clone()));
                
                // Count successes and failures
                if result.is_success() {
                    success_count += 1;
                    tracing::info!("✓ {}: {}ms", id, result.latency.unwrap_or(0));
                } else {
                    failure_count += 1;
                    let category = result.error_category.as_ref()
                        .map(|c| format!("{:?}", c))
                        .unwrap_or_else(|| "Unknown".to_string());
                    tracing::warn!("✗ {}: {} - {}", 
                        id, category,
                        result.error_message.as_ref().unwrap_or(&"Unknown error".to_string()));
                }
            }
            
            tracing::info!("=== Batch test summary: {} successful, {} failed, {} total ===", 
                success_count, failure_count, results.len());
        });
        
        tracing::info!("Batch test initiated, results will be processed as they arrive");
    }
    
    fn connect_endpoint(&mut self, idx: usize) {
        if idx >= self.endpoints.len() {
            return;
        }
        
        // Disconnect current connection if any
        if self.connected_endpoint_id.is_some() {
            self.disconnect_endpoint();
        }
        
        let ep = &self.endpoints[idx].clone();
        
        // Start xray with this endpoint
        if let Some(ref adapter) = self.xray_adapter {
            match self.endpoint_to_config(ep) {
                Ok(endpoint_config) => {
                    match adapter.generate_config(&endpoint_config) {
                        Ok(config) => {
                            if let Err(e) = adapter.write_config(&config) {
                                tracing::error!("Failed to write xray config: {}", e);
                                self.endpoints[idx].test_error = Some(format!("Config error: {}", e));
                                return;
                            }
                            if let Err(e) = adapter.start() {
                                tracing::error!("Failed to start xray: {}", e);
                                self.endpoints[idx].test_error = Some(format!("Failed to start xray: {}", e));
                                return;
                            }
                            tracing::info!("Xray started successfully for endpoint: {}", ep.name);
                        }
                        Err(e) => {
                            tracing::error!("Failed to generate xray config: {}", e);
                            self.endpoints[idx].test_error = Some(format!("Config error: {}", e));
                            return;
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to convert endpoint to config: {}", e);
                    self.endpoints[idx].test_error = Some(format!("Endpoint error: {}", e));
                    return;
                }
            }
        } else {
            tracing::warn!("Xray adapter not available, cannot connect");
            self.endpoints[idx].test_error = Some("Xray not available. Please download it first.".to_string());
            return;
        }
        
        let ep = &mut self.endpoints[idx];
        ep.is_connected = true;
        self.connected_endpoint_id = Some(ep.id.clone());
        
        // Update current connection info
        self.current_connection = Some(ConnectionInfo {
            endpoint_name: ep.name.clone(),
            status: "Connected".to_string(),
            latency: ep.latency,
            bytes_sent: 0,
            bytes_received: 0,
            connected_at: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        });
        
        // Apply system proxy if configured
        if self.client_params.system_proxy && self.proxy_mode != ProxyMode::Unchanged {
            let exceptions: Vec<String> = self.proxy_exceptions
                .split(';')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            
            let config = ProxyConfig {
                mode: self.proxy_mode,
                socks_port: self.socks_port,
                http_port: self.http_port,
                pac_url: if self.proxy_mode == ProxyMode::Pac {
                    Some(format!("http://127.0.0.1:{}/pac", self.http_port))
                } else {
                    None
                },
                exceptions,
                bypass_local: true,
            };
            
            match set_system_proxy(&config) {
                Ok(_) => {
                    tracing::info!("System proxy applied successfully");
                }
                Err(e) => {
                    tracing::error!("Failed to apply system proxy: {}", e);
                    ep.test_error = Some(format!("Proxy setup failed: {}", e));
                }
            }
        }
        
        tracing::info!("Connected to endpoint: {} ({})", ep.name, ep.host);
    }
    
    fn disconnect_endpoint(&mut self) {
        // Stop xray
        if let Some(ref adapter) = self.xray_adapter {
            if let Err(e) = adapter.stop() {
                tracing::error!("Failed to stop xray: {}", e);
            } else {
                tracing::info!("Xray stopped");
            }
        }
        
        if let Some(ref connected_id) = self.connected_endpoint_id {
            // Find and disconnect the endpoint
            for ep in &mut self.endpoints {
                if ep.id == *connected_id {
                    ep.is_connected = false;
                    break;
                }
            }
            
            // Disable system proxy
            if self.client_params.system_proxy {
                let config = ProxyConfig {
                    mode: ProxyMode::Direct,
                    socks_port: self.socks_port,
                    http_port: self.http_port,
                    pac_url: None,
                    exceptions: vec![],
                    bypass_local: true,
                };
                
                if let Err(e) = set_system_proxy(&config) {
                    tracing::error!("Failed to disable system proxy: {}", e);
                }
            }
            
            self.connected_endpoint_id = None;
            self.current_connection = None;
            tracing::info!("Disconnected from endpoint");
        }
    }
    
    fn load_test_config(&mut self) {
        // Test configuration that works with v2rayN
        let test_config_json = r#"
        {
          "log": {
            "loglevel": "warning"
          },
          "dns": {
            "hosts": {
              "dns.google": ["8.8.8.8", "8.8.4.4"],
              "dns.alidns.com": ["223.5.5.5", "223.6.6.6"],
              "one.one.one.one": ["1.1.1.1", "1.0.0.1"],
              "cloudflare-dns.com": ["104.16.249.249", "104.16.248.249"]
            },
            "servers": [
              {
                "address": "https://dns.alidns.com/dns-query",
                "domains": ["zfa01.communet.io"],
                "skipFallback": true
              },
              "https://cloudflare-dns.com/dns-query"
            ]
          },
          "inbounds": [
            {
              "tag": "socks",
              "port": 10810,
              "listen": "127.0.0.1",
              "protocol": "mixed",
              "sniffing": {
                "enabled": true,
                "destOverride": ["http", "tls"],
                "routeOnly": false
              },
              "settings": {
                "auth": "noauth",
                "udp": true,
                "allowTransparent": false
              }
            }
          ],
          "outbounds": [
            {
              "tag": "proxy",
              "protocol": "vless",
              "settings": {
                "vnext": [
                  {
                    "address": "zfa01.communet.io",
                    "port": 40383,
                    "users": [
                      {
                        "id": "0b55d52e-ab08-451f-881d-eb411f0e45cd",
                        "email": "t@t.tt",
                        "security": "auto",
                        "encryption": "none",
                        "flow": "xtls-rprx-vision"
                      }
                    ]
                  }
                ]
              },
              "streamSettings": {
                "network": "tcp",
                "security": "tls",
                "tlsSettings": {
                  "allowInsecure": true,
                  "serverName": "bby.communet.io",
                  "fingerprint": "safari"
                }
              },
              "mux": {
                "enabled": false,
                "concurrency": -1
              }
            },
            {
              "tag": "direct",
              "protocol": "freedom"
            },
            {
              "tag": "block",
              "protocol": "blackhole"
            }
          ],
          "routing": {
            "domainStrategy": "AsIs",
            "rules": [
              {
                "type": "field",
                "outboundTag": "direct",
                "ip": ["geoip:private"]
              },
              {
                "type": "field",
                "outboundTag": "direct",
                "domain": ["geosite:private"]
              },
              {
                "type": "field",
                "port": "0-65535",
                "outboundTag": "proxy"
              }
            ]
          }
        }
        "#;
        
        match serde_json::from_str::<serde_json::Value>(test_config_json) {
            Ok(config) => {
                // Extract endpoint from outbound
                if let Some(outbounds) = config.get("outbounds").and_then(|o| o.as_array()) {
                    if let Some(proxy_outbound) = outbounds.iter().find(|o| {
                        o.get("tag").and_then(|t| t.as_str()) == Some("proxy")
                    }) {
                        if let Some(settings) = proxy_outbound.get("settings") {
                            if let Some(vnext) = settings.get("vnext").and_then(|v| v.as_array()) {
                                if let Some(server) = vnext.first() {
                                    if let Some(users) = server.get("users").and_then(|u| u.as_array()) {
                                        if let Some(user) = users.first() {
                                            let address = server.get("address").and_then(|a| a.as_str()).unwrap_or("");
                                            let port = server.get("port").and_then(|p| p.as_u64()).unwrap_or(0) as u16;
                                            let id = user.get("id").and_then(|i| i.as_str()).unwrap_or("");
                                            let flow = user.get("flow").and_then(|f| f.as_str()).unwrap_or("");
                                            
                                            // Extract TLS settings
                                            let server_name = proxy_outbound
                                                .get("streamSettings")
                                                .and_then(|s| s.get("tlsSettings"))
                                                .and_then(|t| t.get("serverName"))
                                                .and_then(|n| n.as_str());
                                            
                                            let allow_insecure = proxy_outbound
                                                .get("streamSettings")
                                                .and_then(|s| s.get("tlsSettings"))
                                                .and_then(|t| t.get("allowInsecure"))
                                                .and_then(|a| a.as_bool())
                                                .unwrap_or(true);
                                            
                                            // Create endpoint with VLESS configuration
                                            // Store the full outbound config in raw_config for proper parsing
                                            let endpoint = EndpointDisplay {
                                                id: uuid::Uuid::new_v4().to_string(),
                                                name: "Test Config (v2rayN)".to_string(),
                                                host: address.to_string(),
                                                port,
                                                protocol: "vless".to_string(),
                                                tags: vec!["test".to_string(), "v2rayN".to_string()],
                                                enabled: true,
                                                is_connected: false,
                                                raw_config: Some(proxy_outbound.clone()),
                                                latency: None,
                                                testing: false,
                                                test_error: None,
                                                test_diagnostics: vec![],
                                                error_category: None,
                                            };
                                            
                                            // Update Xray settings from config
                                            if let Some(dns) = config.get("dns") {
                                                if let Some(servers) = dns.get("servers").and_then(|s| s.as_array()) {
                                                    let dns_list: Vec<String> = servers
                                                        .iter()
                                                        .filter_map(|s| {
                                                            if let Some(addr) = s.as_str() {
                                                                Some(addr.to_string())
                                                            } else if let Some(obj) = s.as_object() {
                                                                obj.get("address").and_then(|a| a.as_str()).map(|a| a.to_string())
                                                            } else {
                                                                None
                                                            }
                                                        })
                                                        .collect();
                                                    if !dns_list.is_empty() {
                                                        self.xray_settings.dns_servers = dns_list;
                                                    }
                                                }
                                            }
                                            
                                            if let Some(routing) = config.get("routing") {
                                                if let Some(strategy) = routing.get("domainStrategy").and_then(|s| s.as_str()) {
                                                    self.xray_settings.routing_domain_strategy = strategy.to_string();
                                                }
                                            }
                                            
                                            // Update ports from inbound
                                            if let Some(inbounds) = config.get("inbounds").and_then(|i| i.as_array()) {
                                                if let Some(inbound) = inbounds.first() {
                                                    if let Some(port) = inbound.get("port").and_then(|p| p.as_u64()) {
                                                        self.socks_port = port as u16;
                                                    }
                                                }
                                            }
                                            
                                            // Add endpoint
                                            self.endpoints.push(endpoint);
                                            tracing::info!("Loaded test configuration from v2rayN");
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Err(e) => {
                tracing::error!("Failed to parse test config: {}", e);
            }
        }
    }
    
    fn apply_system_proxy(&mut self) {
        if !self.client_params.system_proxy {
            return;
        }
        
        let exceptions: Vec<String> = self.proxy_exceptions
            .split(';')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        
        let config = ProxyConfig {
            mode: self.proxy_mode,
            socks_port: self.socks_port,
            http_port: self.http_port,
            pac_url: if self.proxy_mode == ProxyMode::Pac {
                Some(format!("http://127.0.0.1:{}/pac", self.http_port))
            } else {
                None
            },
            exceptions,
            bypass_local: true,
        };
        
        match set_system_proxy(&config) {
            Ok(_) => {
                tracing::info!("System proxy applied successfully");
                // Could show success message in UI
            }
            Err(e) => {
                tracing::error!("Failed to apply system proxy: {}", e);
                // Could show error message in UI
            }
        }
    }
}

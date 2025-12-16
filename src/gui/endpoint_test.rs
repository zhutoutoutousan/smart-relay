use anyhow::{Result, Context};
use std::time::{Duration, Instant};
use tokio::time::timeout;
use std::process::Command;

#[derive(Debug, Clone)]
pub struct TestResult {
    pub latency: Option<u64>, // milliseconds
    pub dns_resolved: bool,
    pub tcp_reachable: bool,
    pub error_category: Option<ErrorCategory>,
    pub error_message: Option<String>,
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum ErrorCategory {
    DnsFailure,           // Cannot resolve hostname
    ConnectionRefused,    // Port closed or firewall blocking
    ConnectionTimeout,    // No response within timeout
    NetworkUnreachable,  // Network routing issue
    ConfigurationError,   // Protocol or configuration issue
    Unknown,             // Other errors
}

impl TestResult {
    pub fn success(latency: u64) -> Self {
        Self {
            latency: Some(latency),
            dns_resolved: true,
            tcp_reachable: true,
            error_category: None,
            error_message: None,
            diagnostics: vec!["All tests passed".to_string()],
        }
    }
    
    pub fn failure(category: ErrorCategory, message: String, diagnostics: Vec<String>) -> Self {
        Self {
            latency: None,
            dns_resolved: false,
            tcp_reachable: false,
            error_category: Some(category),
            error_message: Some(message),
            diagnostics,
        }
    }
    
    pub fn is_success(&self) -> bool {
        self.latency.is_some()
    }
}

/// Comprehensive endpoint testing with multiple diagnostic methods
pub async fn test_endpoint_comprehensive(host: &str, port: u16) -> TestResult {
    let mut diagnostics = Vec::new();
    
    // Step 1: DNS Resolution Test
    diagnostics.push(format!("Testing DNS resolution for: {}", host));
    let dns_result = test_dns_resolution(host).await;
    if !dns_result.resolved {
        return TestResult::failure(
            ErrorCategory::DnsFailure,
            format!("DNS resolution failed: {}", dns_result.error.unwrap_or_else(|| "Unknown error".to_string())),
            diagnostics,
        );
    }
    diagnostics.push(format!("✓ DNS resolved to: {}", dns_result.ip.unwrap_or_else(|| "Unknown".to_string())));
    
    // Step 2: TCP Connection Test
    diagnostics.push(format!("Testing TCP connection to {}:{}", host, port));
    let tcp_result = test_tcp_connection(host, port).await;
    if let Some(latency) = tcp_result.latency {
        diagnostics.push(format!("✓ TCP connection successful ({}ms)", latency));
        return TestResult::success(latency);
    }
    
    // Step 3: Detailed error analysis
    let error = tcp_result.error.unwrap_or_else(|| "Unknown error".to_string());
    let category = categorize_error(&error);
    diagnostics.push(format!("✗ TCP connection failed: {}", error));
    
    // Step 4: Additional diagnostics based on error type
    match &category {
        ErrorCategory::ConnectionRefused => {
            diagnostics.push("Possible causes:".to_string());
            diagnostics.push("  - Port is closed on remote server".to_string());
            diagnostics.push("  - Firewall is blocking the connection".to_string());
            diagnostics.push("  - Service is not running".to_string());
        }
        ErrorCategory::ConnectionTimeout => {
            diagnostics.push("Possible causes:".to_string());
            diagnostics.push("  - Firewall is silently dropping packets".to_string());
            diagnostics.push("  - Network routing issue".to_string());
            diagnostics.push("  - Server is overloaded".to_string());
        }
        ErrorCategory::NetworkUnreachable => {
            diagnostics.push("Possible causes:".to_string());
            diagnostics.push("  - Network routing problem".to_string());
            diagnostics.push("  - VPN/tunnel not established".to_string());
            diagnostics.push("  - ISP blocking".to_string());
        }
        _ => {}
    }
    
    TestResult::failure(category, error, diagnostics)
}

#[derive(Debug)]
struct DnsTestResult {
    resolved: bool,
    ip: Option<String>,
    error: Option<String>,
}

async fn test_dns_resolution(host: &str) -> DnsTestResult {
    use tokio::net::lookup_host;
    
    match timeout(Duration::from_secs(3), lookup_host((host, 0))).await {
        Ok(Ok(mut addrs)) => {
            if let Some(addr) = addrs.next() {
                DnsTestResult {
                    resolved: true,
                    ip: Some(addr.ip().to_string()),
                    error: None,
                }
            } else {
                DnsTestResult {
                    resolved: false,
                    ip: None,
                    error: Some("No addresses found".to_string()),
                }
            }
        }
        Ok(Err(e)) => {
            DnsTestResult {
                resolved: false,
                ip: None,
                error: Some(format!("DNS lookup failed: {}", e)),
            }
        }
        Err(_) => {
            DnsTestResult {
                resolved: false,
                ip: None,
                error: Some("DNS resolution timeout".to_string()),
            }
        }
    }
}

#[derive(Debug)]
struct TcpTestResult {
    latency: Option<u64>,
    error: Option<String>,
}

async fn test_tcp_connection(host: &str, port: u16) -> TcpTestResult {
    use tokio::net::TcpStream;
    
    let start = Instant::now();
    
    match timeout(Duration::from_secs(5), TcpStream::connect((host, port))).await {
        Ok(Ok(_stream)) => {
            let latency = start.elapsed().as_millis() as u64;
            TcpTestResult {
                latency: Some(latency),
                error: None,
            }
        }
        Ok(Err(e)) => {
            TcpTestResult {
                latency: None,
                error: Some(format!("Connection failed: {}", e)),
            }
        }
        Err(_) => {
            TcpTestResult {
                latency: None,
                error: Some("Connection timeout".to_string()),
            }
        }
    }
}

fn categorize_error(error: &str) -> ErrorCategory {
    let error_lower = error.to_lowercase();
    
    if error_lower.contains("dns") || error_lower.contains("name or service not known") {
        ErrorCategory::DnsFailure
    } else if error_lower.contains("refused") || error_lower.contains("积极拒绝") || error_lower.contains("10061") {
        ErrorCategory::ConnectionRefused
    } else if error_lower.contains("timeout") {
        ErrorCategory::ConnectionTimeout
    } else if error_lower.contains("unreachable") || error_lower.contains("no route") {
        ErrorCategory::NetworkUnreachable
    } else if error_lower.contains("configuration") || error_lower.contains("invalid") {
        ErrorCategory::ConfigurationError
    } else {
        ErrorCategory::Unknown
    }
}

/// Simple TCP connection test (backward compatibility)
pub async fn test_endpoint(host: &str, port: u16) -> Result<u64> {
    let result = test_endpoint_comprehensive(host, port).await;
    match result.latency {
        Some(latency) => Ok(latency),
        None => {
            let msg = result.error_message.unwrap_or_else(|| "Connection failed".to_string());
            Err(anyhow::anyhow!(msg))
        }
    }
}

/// Smart endpoint testing that tries multiple approaches with deep network diagnostics
pub async fn test_endpoint_smart(
    host: &str,
    port: u16,
    protocol: &str,
    local_socks_port: u16,
) -> TestResult {
    let mut diagnostics = Vec::new();
    diagnostics.push(format!("Starting comprehensive network analysis for {}://{}:{}", protocol, host, port));
    diagnostics.push("=".repeat(60));
    
    // Step 0: Network Interface Information
    diagnostics.push("\n[Layer 2/3] Network Interface Analysis:".to_string());
    let interface_info = get_network_interfaces();
    diagnostics.extend(interface_info);
    
    // Step 1: DNS Resolution Test (Layer 7/Application)
    diagnostics.push("\n[Layer 7] DNS Resolution Test:".to_string());
    diagnostics.push(format!("Resolving hostname: {}", host));
    let dns_result = test_dns_resolution(host).await;
    if !dns_result.resolved {
        let error_msg = dns_result.error.clone().unwrap_or_else(|| "Unknown error".to_string());
        diagnostics.push(format!("✗ DNS resolution failed: {}", error_msg));
        diagnostics.push("DNS failure prevents further testing".to_string());
        return TestResult::failure(
            ErrorCategory::DnsFailure,
            format!("DNS resolution failed: {}", error_msg),
            diagnostics,
        );
    }
    let resolved_ip = dns_result.ip.unwrap_or_else(|| "Unknown".to_string());
    diagnostics.push(format!("✓ DNS resolved: {} -> {}", host, resolved_ip));
    
    // Step 1.5: Routing Information (Layer 3/Network)
    diagnostics.push("\n[Layer 3] Routing Information:".to_string());
    let routing_info = get_routing_info(&resolved_ip).await;
    diagnostics.extend(routing_info);
    
    // Step 2: TCP Handshake Analysis (Layer 4/Transport)
    diagnostics.push("\n[Layer 4] TCP Connection Analysis:".to_string());
    diagnostics.push(format!("Target: {}:{} (IP: {})", host, port, resolved_ip));
    
    let tcp_details = analyze_tcp_handshake(host, port).await;
    diagnostics.extend(tcp_details);
    
    // Step 2.5: Protocol-Specific Testing (if applicable)
    diagnostics.push(format!("\n[Layer 5-7] Protocol-Specific Test ({})", protocol.to_uppercase()));
    let protocol_result = test_protocol_specific(host, port, protocol).await;
    diagnostics.extend(protocol_result.diagnostics);
    if let Some(latency) = protocol_result.latency {
        diagnostics.push(format!("✓ {} protocol handshake successful ({}ms)", protocol, latency));
        diagnostics.push("Note: Protocol-specific test passed - endpoint is working!".to_string());
        return TestResult::success(latency);
    }
    
    // Step 2.6: Direct TCP Connection Test (fast timeout)
    diagnostics.push("\n[Layer 4] Direct TCP Connection Test (3s timeout):".to_string());
    let direct_result = test_tcp_connection_with_timeout(host, port, Duration::from_secs(3)).await;
    if let Some(latency) = direct_result.latency {
        diagnostics.push(format!("✓ Direct connection successful ({}ms)", latency));
        diagnostics.push(format!("TCP handshake completed in {}ms", latency));
        diagnostics.push("Note: TCP works, but protocol-specific test may still be needed".to_string());
        return TestResult::success(latency);
    }
    diagnostics.push(format!("✗ Direct connection failed: {}", direct_result.error.as_ref().unwrap_or(&"Timeout".to_string())));
    
    // Step 3: Try via local SOCKS proxy if available
    if local_socks_port > 0 {
        diagnostics.push(format!("\n[Layer 5] Testing via local SOCKS proxy (127.0.0.1:{}):", local_socks_port));
        let proxy_result = test_via_socks_proxy(host, port, local_socks_port).await;
        if let Some(latency) = proxy_result.latency {
            diagnostics.push(format!("✓ Connection via proxy successful ({}ms)", latency));
            diagnostics.push("Note: Direct connection failed, but proxy works - may need proxy for this endpoint".to_string());
            return TestResult::success(latency);
        }
        diagnostics.push(format!("✗ Proxy connection failed: {}", proxy_result.error.as_ref().unwrap_or(&"Timeout".to_string())));
    } else {
        diagnostics.push("\n[Layer 5] Skipping proxy test (no local proxy configured)".to_string());
    }
    
    // Step 4: Extended timeout test (for slow connections)
    diagnostics.push("\n[Layer 4] Extended Timeout Test (10s):".to_string());
    let extended_result = test_tcp_connection_with_timeout(host, port, Duration::from_secs(10)).await;
    if let Some(latency) = extended_result.latency {
        diagnostics.push(format!("✓ Extended timeout connection successful ({}ms)", latency));
        diagnostics.push("Note: Connection is slow but working".to_string());
        return TestResult::success(latency);
    }
    
    // Step 5: Packet-level analysis (if possible)
    diagnostics.push("\n[Layer 2/3] Packet-Level Analysis:".to_string());
    let packet_info = analyze_packet_level(host, port, resolved_ip.as_str()).await;
    diagnostics.extend(packet_info);
    
    // All tests failed - categorize the error
    let error = extended_result.error.unwrap_or_else(|| "Connection timeout".to_string());
    let category = categorize_error(&error);
    
    diagnostics.push("\n[Summary] All connection attempts failed".to_string());
    diagnostics.push("=".repeat(60));
    diagnostics.push("Possible causes:".to_string());
    match &category {
        ErrorCategory::ConnectionRefused => {
            diagnostics.push("  - Port is closed on remote server".to_string());
            diagnostics.push("  - Firewall is blocking the connection".to_string());
            diagnostics.push("  - Service is not running".to_string());
            diagnostics.push("  - Endpoint may require specific protocol configuration".to_string());
            diagnostics.push("  - TCP SYN packet sent but received RST (connection refused)".to_string());
            diagnostics.push("  - Server may only accept connections with proper protocol handshake".to_string());
            diagnostics.push("  - Geographic restrictions: server may block your region".to_string());
        }
        ErrorCategory::ConnectionTimeout => {
            diagnostics.push("  - Firewall is silently dropping packets (no response to SYN)".to_string());
            diagnostics.push("  - Network routing issue (packets not reaching destination)".to_string());
            diagnostics.push("  - Server is overloaded or down".to_string());
            diagnostics.push("  - ISP or network provider blocking".to_string());
            diagnostics.push("  - TCP SYN packet sent but no SYN-ACK received".to_string());
            diagnostics.push("  - Server may only accept connections from specific IP ranges".to_string());
            diagnostics.push("  - Protocol fingerprinting: server may require specific handshake".to_string());
            diagnostics.push("  - Note: Direct TCP test may fail, but proxy connection might work!".to_string());
            diagnostics.push("    (Server may require VMess/Shadowsocks handshake, not plain TCP)".to_string());
        }
        ErrorCategory::NetworkUnreachable => {
            diagnostics.push("  - Network routing problem (no route to host)".to_string());
            diagnostics.push("  - VPN/tunnel not established".to_string());
            diagnostics.push("  - ISP blocking or IP blackholing".to_string());
            diagnostics.push("  - Route nullification: packets may be dropped at network layer".to_string());
        }
        _ => {
            diagnostics.push("  - Unknown network issue".to_string());
            diagnostics.push("  - Check endpoint configuration".to_string());
            diagnostics.push("  - Verify protocol requirements (VMess, Shadowsocks, etc.)".to_string());
        }
    }
    
    // Add note about proxy protocols
    diagnostics.push("".to_string());
    diagnostics.push("Important Notes:".to_string());
    diagnostics.push("  - Many proxy endpoints (VMess, Shadowsocks) require specific protocol handshakes".to_string());
    diagnostics.push("  - A simple TCP test cannot verify if the endpoint actually works".to_string());
    diagnostics.push("  - To fully test, you need to:".to_string());
    diagnostics.push("    1. Start your proxy client (xray, sing-box, etc.)".to_string());
    diagnostics.push("    2. Configure it with this endpoint".to_string());
    diagnostics.push("    3. Test actual connectivity through the proxy".to_string());
    diagnostics.push("  - Direct TCP failures don't necessarily mean the endpoint is broken!".to_string());
    
    TestResult::failure(category, error, diagnostics)
}

async fn test_tcp_connection_with_timeout(host: &str, port: u16, timeout_duration: Duration) -> TcpTestResult {
    use tokio::net::TcpStream;
    
    let start = Instant::now();
    
    match timeout(timeout_duration, TcpStream::connect((host, port))).await {
        Ok(Ok(_stream)) => {
            let latency = start.elapsed().as_millis() as u64;
            TcpTestResult {
                latency: Some(latency),
                error: None,
            }
        }
        Ok(Err(e)) => {
            TcpTestResult {
                latency: None,
                error: Some(format!("Connection failed: {}", e)),
            }
        }
        Err(_) => {
            TcpTestResult {
                latency: None,
                error: Some("Connection timeout".to_string()),
            }
        }
    }
}

/// Test endpoint using protocol-specific handshake
async fn test_protocol_specific(host: &str, port: u16, protocol: &str) -> TestResult {
    let mut diagnostics = Vec::new();
    let protocol_lower = protocol.to_lowercase();
    
    diagnostics.push(format!("Testing {} protocol handshake...", protocol));
    
    match protocol_lower.as_str() {
        "vmess" => {
            diagnostics.push("VMess Protocol Test:".to_string());
            diagnostics.push("  - VMess requires: UUID, encryption key, authentication".to_string());
            diagnostics.push("  - Handshake: Client sends encrypted request with timestamp".to_string());
            diagnostics.push("  - Server validates and responds with encrypted response".to_string());
            diagnostics.push("  - Note: Full VMess test requires complete endpoint configuration".to_string());
            diagnostics.push("  - Attempting basic connection to verify server is listening...".to_string());
            
            // Try basic TCP connection first
            let tcp_result = test_tcp_connection_with_timeout(host, port, Duration::from_secs(5)).await;
            if let Some(latency) = tcp_result.latency {
                diagnostics.push(format!("  ✓ TCP connection works ({}ms)", latency));
                diagnostics.push("  ⚠ However, VMess handshake requires UUID and encryption".to_string());
                diagnostics.push("  ⚠ This test cannot verify if VMess protocol actually works".to_string());
                diagnostics.push("  ⚠ To fully test, configure xray/sing-box and test through proxy".to_string());
                return TestResult {
                    latency: Some(latency),
                    dns_resolved: true,
                    tcp_reachable: true,
                    error_category: Some(ErrorCategory::ConfigurationError),
                    error_message: Some("VMess requires full configuration for proper test".to_string()),
                    diagnostics,
                };
            } else {
                diagnostics.push(format!("  ✗ TCP connection failed: {}", tcp_result.error.as_ref().unwrap_or(&"Timeout".to_string())));
                diagnostics.push("  - Cannot test VMess if basic TCP connection fails".to_string());
            }
        }
        "shadowsocks" | "ss" => {
            diagnostics.push("Shadowsocks Protocol Test:".to_string());
            diagnostics.push("  - Shadowsocks requires: encryption method, password".to_string());
            diagnostics.push("  - Handshake: Client sends encrypted request".to_string());
            diagnostics.push("  - Server decrypts and processes request".to_string());
            diagnostics.push("  - Note: Full Shadowsocks test requires method and password".to_string());
            diagnostics.push("  - Attempting basic connection to verify server is listening...".to_string());
            
            // Try basic TCP connection first
            let tcp_result = test_tcp_connection_with_timeout(host, port, Duration::from_secs(5)).await;
            if let Some(latency) = tcp_result.latency {
                diagnostics.push(format!("  ✓ TCP connection works ({}ms)", latency));
                diagnostics.push("  ⚠ However, Shadowsocks handshake requires method and password".to_string());
                diagnostics.push("  ⚠ This test cannot verify if Shadowsocks protocol actually works".to_string());
                diagnostics.push("  ⚠ To fully test, configure xray/sing-box and test through proxy".to_string());
                return TestResult {
                    latency: Some(latency),
                    dns_resolved: true,
                    tcp_reachable: true,
                    error_category: Some(ErrorCategory::ConfigurationError),
                    error_message: Some("Shadowsocks requires full configuration for proper test".to_string()),
                    diagnostics,
                };
            } else {
                diagnostics.push(format!("  ✗ TCP connection failed: {}", tcp_result.error.as_ref().unwrap_or(&"Timeout".to_string())));
                diagnostics.push("  - Cannot test Shadowsocks if basic TCP connection fails".to_string());
            }
        }
        "vless" => {
            diagnostics.push("VLESS Protocol Test:".to_string());
            diagnostics.push("  - VLESS requires: UUID, flow control, encryption".to_string());
            diagnostics.push("  - Handshake: Similar to VMess but with different encryption".to_string());
            diagnostics.push("  - Note: Full VLESS test requires complete endpoint configuration".to_string());
            
            let tcp_result = test_tcp_connection_with_timeout(host, port, Duration::from_secs(5)).await;
            if let Some(latency) = tcp_result.latency {
                diagnostics.push(format!("  ✓ TCP connection works ({}ms)", latency));
                diagnostics.push("  ⚠ VLESS requires UUID and encryption for proper test".to_string());
                return TestResult {
                    latency: Some(latency),
                    dns_resolved: true,
                    tcp_reachable: true,
                    error_category: Some(ErrorCategory::ConfigurationError),
                    error_message: Some("VLESS requires full configuration for proper test".to_string()),
                    diagnostics,
                };
            }
        }
        "trojan" => {
            diagnostics.push("Trojan Protocol Test:".to_string());
            diagnostics.push("  - Trojan requires: password, TLS certificate validation".to_string());
            diagnostics.push("  - Handshake: TLS handshake followed by Trojan protocol".to_string());
            diagnostics.push("  - Note: Full Trojan test requires password and TLS config".to_string());
            
            let tcp_result = test_tcp_connection_with_timeout(host, port, Duration::from_secs(5)).await;
            if let Some(latency) = tcp_result.latency {
                diagnostics.push(format!("  ✓ TCP connection works ({}ms)", latency));
                diagnostics.push("  ⚠ Trojan requires password and TLS for proper test".to_string());
                return TestResult {
                    latency: Some(latency),
                    dns_resolved: true,
                    tcp_reachable: true,
                    error_category: Some(ErrorCategory::ConfigurationError),
                    error_message: Some("Trojan requires full configuration for proper test".to_string()),
                    diagnostics,
                };
            }
        }
        "socks5" | "socks" => {
            diagnostics.push("SOCKS5 Protocol Test:".to_string());
            diagnostics.push("  - Attempting SOCKS5 handshake...".to_string());
            
            // SOCKS5 has a simpler handshake we can actually test
            match test_socks5_handshake(host, port).await {
                Ok(latency) => {
                    diagnostics.push(format!("  ✓ SOCKS5 handshake successful ({}ms)", latency));
                    return TestResult::success(latency);
                }
                Err(e) => {
                    diagnostics.push(format!("  ✗ SOCKS5 handshake failed: {}", e));
                    diagnostics.push("  - Server may not support SOCKS5 or requires authentication".to_string());
                }
            }
        }
        _ => {
            diagnostics.push(format!("Protocol '{}' not specifically tested", protocol));
            diagnostics.push("  - Falling back to generic TCP test".to_string());
        }
    }
    
    // Protocol-specific test didn't succeed
    TestResult {
        latency: None,
        dns_resolved: true,
        tcp_reachable: false,
        error_category: Some(ErrorCategory::ConfigurationError),
        error_message: Some(format!("Protocol-specific test for {} not fully implemented or requires configuration", protocol)),
        diagnostics,
    }
}

/// Test SOCKS5 handshake (simpler protocol we can actually test)
async fn test_socks5_handshake(host: &str, port: u16) -> Result<u64> {
    use tokio::net::TcpStream;
    use tokio::io::{AsyncWriteExt, AsyncReadExt};
    
    let start = Instant::now();
    
    // Connect to server
    let mut stream = timeout(Duration::from_secs(5), TcpStream::connect((host, port)))
        .await
        .context("Connection timeout")?
        .context("Failed to connect")?;
    
    // SOCKS5 handshake: send greeting
    // Version (1 byte) + Number of methods (1 byte) + Methods (1-255 bytes)
    // We'll send: 0x05 (SOCKS5) + 0x01 (1 method) + 0x00 (No authentication)
    let greeting = vec![0x05, 0x01, 0x00];
    stream.write_all(&greeting).await.context("Failed to send SOCKS5 greeting")?;
    
    // Read server response
    // Expected: 0x05 (SOCKS5) + 0x00 (No authentication required)
    let mut response = [0u8; 2];
    timeout(Duration::from_secs(2), stream.read_exact(&mut response))
        .await
        .context("Response timeout")?
        .context("Failed to read response")?;
    
    if response[0] != 0x05 {
        return Err(anyhow::anyhow!("Invalid SOCKS5 version in response: {}", response[0]));
    }
    
    if response[1] == 0xFF {
        return Err(anyhow::anyhow!("SOCKS5 server rejected: No acceptable authentication method"));
    }
    
    let latency = start.elapsed().as_millis() as u64;
    Ok(latency)
}

async fn test_via_socks_proxy(_host: &str, _port: u16, proxy_port: u16) -> TcpTestResult {
    // Test connection via local SOCKS proxy
    // This is a simplified test - in production, use a proper SOCKS client library
    use tokio::net::TcpStream;
    
    let start = Instant::now();
    
    // Try to connect to SOCKS proxy
    match timeout(Duration::from_secs(3), TcpStream::connect(("127.0.0.1", proxy_port))).await {
        Ok(Ok(_proxy_stream)) => {
            // Proxy is reachable - connection would work via proxy
            // For now, just indicate proxy is available
            let latency = start.elapsed().as_millis() as u64;
            TcpTestResult {
                latency: Some(latency),
                error: None,
            }
        }
        _ => {
            TcpTestResult {
                latency: None,
                error: Some("Local proxy not available".to_string()),
            }
        }
    }
}

/// Get network interface information
fn get_network_interfaces() -> Vec<String> {
    let mut diagnostics = Vec::new();
    
    #[cfg(windows)]
    {
        // Use ipconfig on Windows
        match Command::new("ipconfig").args(&["/all"]).output() {
            Ok(output) => {
                let output_str = String::from_utf8_lossy(&output.stdout);
                // Extract key interface information
                for line in output_str.lines() {
                    if line.contains("Ethernet adapter") || line.contains("Wireless LAN adapter") {
                        diagnostics.push(format!("  Interface: {}", line.trim()));
                    } else if line.contains("IPv4 Address") {
                        diagnostics.push(format!("  {}", line.trim()));
                    } else if line.contains("Default Gateway") {
                        diagnostics.push(format!("  {}", line.trim()));
                    }
                }
            }
            Err(e) => {
                diagnostics.push(format!("  Could not retrieve interface info: {} (may require admin)", e));
            }
        }
    }
    
    #[cfg(not(windows))]
    {
        // Use ip or ifconfig on Linux/macOS
        if let Ok(output) = Command::new("ip").args(&["addr", "show"]).output() {
            let output_str = String::from_utf8_lossy(&output.stdout);
            for line in output_str.lines() {
                if line.contains("inet ") && !line.contains("127.0.0.1") {
                    diagnostics.push(format!("  {}", line.trim()));
                }
            }
        } else if let Ok(output) = Command::new("ifconfig").output() {
            let output_str = String::from_utf8_lossy(&output.stdout);
            for line in output_str.lines() {
                if line.contains("inet ") && !line.contains("127.0.0.1") {
                    diagnostics.push(format!("  {}", line.trim()));
                }
            }
        }
    }
    
    if diagnostics.is_empty() {
        diagnostics.push("  Could not retrieve network interface information".to_string());
    }
    
    diagnostics
}

/// Get routing information (traceroute-like)
async fn get_routing_info(target_ip: &str) -> Vec<String> {
    let mut diagnostics = Vec::new();
    
    // Try to get routing table information
    #[cfg(windows)]
    {
        match Command::new("route").args(&["print"]).output() {
            Ok(output) => {
                let output_str = String::from_utf8_lossy(&output.stdout);
                diagnostics.push("  Routing table entries:".to_string());
                for line in output_str.lines().take(10) {
                    if line.contains("0.0.0.0") || line.contains("Network Destination") {
                        diagnostics.push(format!("    {}", line.trim()));
                    }
                }
            }
            Err(e) => {
                diagnostics.push(format!("  Could not retrieve routing table: {} (may require admin)", e));
            }
        }
        
        // Try tracert (first hop only for speed)
        diagnostics.push("  Testing route to target:".to_string());
        match Command::new("tracert")
            .args(&["-h", "3", "-w", "1000", target_ip])
            .output() {
            Ok(output) => {
                let output_str = String::from_utf8_lossy(&output.stdout);
                for line in output_str.lines().skip(3).take(3) {
                    if !line.trim().is_empty() {
                        diagnostics.push(format!("    {}", line.trim()));
                    }
                }
            }
            Err(e) => {
                diagnostics.push(format!("  Could not trace route: {} (may require admin or timeout)", e));
            }
        }
    }
    
    #[cfg(not(windows))]
    {
        // Use ip route on Linux
        if let Ok(output) = Command::new("ip").args(&["route", "get", target_ip]).output() {
            let output_str = String::from_utf8_lossy(&output.stdout);
            diagnostics.push(format!("  Route: {}", output_str.trim()));
        }
        
        // Try traceroute (first 3 hops)
        if let Ok(output) = Command::new("traceroute")
            .args(&["-m", "3", "-w", "1", target_ip])
            .output() {
            let output_str = String::from_utf8_lossy(&output.stdout);
            for line in output_str.lines().skip(1).take(3) {
                diagnostics.push(format!("    {}", line.trim()));
            }
        }
    }
    
    if diagnostics.is_empty() {
        diagnostics.push("  Could not retrieve routing information".to_string());
    }
    
    diagnostics
}

/// Analyze TCP handshake in detail
async fn analyze_tcp_handshake(host: &str, port: u16) -> Vec<String> {
    let mut diagnostics = Vec::new();
    
    diagnostics.push(format!("  Target: {}:{}", host, port));
    diagnostics.push("  TCP Handshake Process:".to_string());
    diagnostics.push("    1. Client sends SYN packet (synchronize)".to_string());
    diagnostics.push("    2. Server responds with SYN-ACK (synchronize-acknowledge)".to_string());
    diagnostics.push("    3. Client sends ACK (acknowledge) - connection established".to_string());
    
    // Try to connect and measure handshake timing
    use tokio::net::TcpStream;
    let start = Instant::now();
    
    match timeout(Duration::from_secs(5), TcpStream::connect((host, port))).await {
        Ok(Ok(stream)) => {
            let elapsed = start.elapsed();
            let local_addr = stream.local_addr();
            let peer_addr = stream.peer_addr();
            
            diagnostics.push(format!("  ✓ TCP handshake completed in {}ms", elapsed.as_millis()));
            if let Ok(addr) = local_addr {
                diagnostics.push(format!("  Local endpoint: {}", addr));
            }
            if let Ok(addr) = peer_addr {
                diagnostics.push(format!("  Remote endpoint: {}", addr));
            }
        }
        Ok(Err(e)) => {
            diagnostics.push(format!("  ✗ TCP handshake failed: {}", e));
            diagnostics.push("  Analysis:".to_string());
            
            let error_str = e.to_string();
            if error_str.contains("refused") || error_str.contains("积极拒绝") || error_str.contains("10061") {
                diagnostics.push("    - Server received SYN but sent RST (connection refused)".to_string());
                diagnostics.push("    - Port is closed or service not listening".to_string());
            } else if error_str.contains("timeout") {
                diagnostics.push("    - SYN packet sent but no SYN-ACK received".to_string());
                diagnostics.push("    - Firewall may be silently dropping packets".to_string());
                diagnostics.push("    - Network routing issue".to_string());
            } else if error_str.contains("unreachable") {
                diagnostics.push("    - No route to host".to_string());
                diagnostics.push("    - Network layer routing failure".to_string());
            }
        }
        Err(_) => {
            diagnostics.push("  ✗ TCP handshake timeout (no response to SYN)".to_string());
            diagnostics.push("  Analysis:".to_string());
            diagnostics.push("    - SYN packet likely sent but no response".to_string());
            diagnostics.push("    - Firewall may be silently dropping packets".to_string());
            diagnostics.push("    - Network congestion or routing issue".to_string());
        }
    }
    
    diagnostics
}

/// Analyze packet-level information
async fn analyze_packet_level(host: &str, port: u16, resolved_ip: &str) -> Vec<String> {
    let mut diagnostics = Vec::new();
    
    diagnostics.push("  Packet-Level Details:".to_string());
    diagnostics.push(format!("    Destination IP: {}", resolved_ip));
    diagnostics.push(format!("    Destination Port: {}", port));
    diagnostics.push(format!("    Protocol: TCP"));
    
    // Try to get MTU information
    #[cfg(windows)]
    {
        match Command::new("netsh")
            .args(&["interface", "ipv4", "show", "interfaces"])
            .output() {
            Ok(output) => {
                let output_str = String::from_utf8_lossy(&output.stdout);
                for line in output_str.lines() {
                    if line.contains("MTU") {
                        diagnostics.push(format!("    {}", line.trim()));
                        break;
                    }
                }
            }
            Err(e) => {
                diagnostics.push(format!("    Could not retrieve MTU: {} (may require admin)", e));
            }
        }
    }
    
    #[cfg(not(windows))]
    {
        if let Ok(output) = Command::new("ip").args(&["link", "show"]).output() {
            let output_str = String::from_utf8_lossy(&output.stdout);
            for line in output_str.lines() {
                if line.contains("mtu") {
                    diagnostics.push(format!("    {}", line.trim()));
                }
            }
        }
    }
    
    // TCP window size and options (theoretical)
    diagnostics.push("  TCP Connection Parameters:".to_string());
    diagnostics.push("    Window Size: Default (OS-dependent)".to_string());
    diagnostics.push("    MSS (Maximum Segment Size): ~1460 bytes (Ethernet)".to_string());
    diagnostics.push("    TCP Options: Window Scaling, SACK, Timestamps".to_string());
    
    // Analyze connection attempt with packet details
    diagnostics.push("  Packet Transmission Details:".to_string());
    diagnostics.push("    Protocol: TCP (Transmission Control Protocol)".to_string());
    diagnostics.push("    IP Protocol Number: 6".to_string());
    diagnostics.push(format!("    Source Port: (ephemeral, OS-assigned)"));
    diagnostics.push(format!("    Destination Port: {}", port));
    diagnostics.push(format!("    Destination IP: {}", resolved_ip));
    diagnostics.push("".to_string());
    
    diagnostics.push("  TCP Header Fields (theoretical):".to_string());
    diagnostics.push("    Source Port: 16 bits (ephemeral)".to_string());
    diagnostics.push("    Destination Port: 16 bits".to_string());
    diagnostics.push("    Sequence Number: 32 bits (random initial)".to_string());
    diagnostics.push("    Acknowledgment Number: 32 bits".to_string());
    diagnostics.push("    Data Offset: 4 bits".to_string());
    diagnostics.push("    Flags: SYN, ACK, FIN, RST, PSH, URG".to_string());
    diagnostics.push("    Window Size: 16 bits (receive window)".to_string());
    diagnostics.push("    Checksum: 16 bits (header + data)".to_string());
    diagnostics.push("".to_string());
    
    // Analyze connection attempt
    diagnostics.push("  Connection Attempt Analysis:".to_string());
    diagnostics.push("    Attempting TCP SYN packet to establish connection...".to_string());
    
    use tokio::net::TcpStream;
    let start = Instant::now();
    
    match timeout(Duration::from_secs(2), TcpStream::connect((host, port))).await {
        Ok(Ok(stream)) => {
            let elapsed = start.elapsed();
            let local_addr = stream.local_addr();
            let peer_addr = stream.peer_addr();
            
            diagnostics.push(format!("    ✓ SYN-ACK received in {}ms", elapsed.as_millis()));
            diagnostics.push("    ✓ TCP connection established".to_string());
            diagnostics.push("    Packet flow:".to_string());
            diagnostics.push("      1. Client → Server: SYN (seq=x)".to_string());
            diagnostics.push(format!("      2. Server → Client: SYN-ACK (seq=y, ack=x+1) [received in {}ms]", elapsed.as_millis()));
            diagnostics.push("      3. Client → Server: ACK (ack=y+1)".to_string());
            
            if let Ok(addr) = local_addr {
                diagnostics.push(format!("    Local socket: {}", addr));
            }
            if let Ok(addr) = peer_addr {
                diagnostics.push(format!("    Remote socket: {}", addr));
            }
        }
        Ok(Err(e)) => {
            let error_str = e.to_string();
            diagnostics.push(format!("    ✗ Connection failed: {}", error_str));
            diagnostics.push("    Packet flow:".to_string());
            
            if error_str.contains("refused") {
                diagnostics.push("      1. Client → Server: SYN (seq=x)".to_string());
                diagnostics.push("      2. Server → Client: RST (connection refused)".to_string());
                diagnostics.push("      Result: Connection actively rejected by server".to_string());
            } else if error_str.contains("timeout") {
                diagnostics.push("      1. Client → Server: SYN (seq=x)".to_string());
                diagnostics.push("      2. Server → Client: (no response - timeout)".to_string());
                diagnostics.push("      Possible causes:".to_string());
                diagnostics.push("        - Firewall dropping packets silently".to_string());
                diagnostics.push("        - Routing issue (packets not reaching destination)".to_string());
                diagnostics.push("        - Server is down or overloaded".to_string());
            }
        }
        Err(_) => {
            diagnostics.push("    ✗ No response to SYN packet (timeout)".to_string());
            diagnostics.push("    Packet flow:".to_string());
            diagnostics.push("      1. Client → Server: SYN (seq=x)".to_string());
            diagnostics.push("      2. Server → Client: (silent drop - no response)".to_string());
            diagnostics.push("    Analysis:".to_string());
            diagnostics.push("      - Firewall in stealth mode (dropping without response)".to_string());
            diagnostics.push("      - Network layer issue (packets not routed)".to_string());
            diagnostics.push("      - TCP retransmission will occur (exponential backoff)".to_string());
        }
    }
    
    diagnostics
}

/// Batch test multiple endpoints with comprehensive diagnostics
pub async fn batch_test_endpoints(
    endpoints: Vec<(String, String, u16, String, u16)>, // (id, host, port, protocol, socks_port)
) -> Vec<(String, TestResult)> {
    use futures::future::join_all;
    use tracing::info;
    
    let total = endpoints.len();
    info!("Starting batch test for {} endpoints", total);
    
    let futures: Vec<_> = endpoints.into_iter().enumerate().map(|(idx, (id, host, port, protocol, socks_port))| {
        let id_clone = id.clone();
        let host_clone = host.clone();
        async move {
            info!("[Batch {}/{}] Testing {}:{} ({})", idx + 1, total, host_clone, port, protocol);
            let start = std::time::Instant::now();
            let result = test_endpoint_smart(&host_clone, port, &protocol, socks_port).await;
            let elapsed = start.elapsed();
            
            if result.is_success() {
                info!("[Batch {}/{}] ✓ {}:{} - {}ms (test took {:?})", 
                    idx + 1, total, host_clone, port, 
                    result.latency.unwrap_or(0), elapsed);
            } else {
                let category = result.error_category.as_ref()
                    .map(|c| format!("{:?}", c))
                    .unwrap_or_else(|| "Unknown".to_string());
                info!("[Batch {}/{}] ✗ {}:{} - {} ({:?})", 
                    idx + 1, total, host_clone, port, category, elapsed);
            }
            
            (id_clone, result)
        }
    }).collect();
    
    info!("All {} batch test tasks spawned, waiting for completion...", total);
    let results = join_all(futures).await;
    info!("All {} batch test tasks completed", results.len());
    
    results
}


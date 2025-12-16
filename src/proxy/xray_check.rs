use anyhow::Result;
use std::time::Duration;
use tokio::time::timeout;

/// Check if Xray core is running by testing the API endpoint
pub async fn check_xray_running(api_port: u16) -> Result<bool> {
    use reqwest::Client;
    
    let client = Client::builder()
        .timeout(Duration::from_secs(2))
        .build()?;
    
    // Try to connect to Xray API (common ports: 10085, 10086, or custom)
    let url = format!("http://127.0.0.1:{}/stats", api_port);
    
    match timeout(Duration::from_secs(2), client.get(&url).send()).await {
        Ok(Ok(response)) => {
            Ok(response.status().is_success())
        }
        _ => {
            // Try alternative: check if process is running
            check_xray_process().await
        }
    }
}

/// Check if Xray process is running (Windows-specific for now)
#[cfg(windows)]
async fn check_xray_process() -> Result<bool> {
    use std::process::Command;
    
    let output = Command::new("tasklist")
        .args(&["/FI", "IMAGENAME eq xray.exe"])
        .output()?;
    
    let output_str = String::from_utf8_lossy(&output.stdout);
    Ok(output_str.contains("xray.exe"))
}

#[cfg(not(windows))]
async fn check_xray_process() -> Result<bool> {
    use std::process::Command;
    
    let output = Command::new("pgrep")
        .arg("-x")
        .arg("xray")
        .output()?;
    
    Ok(output.status.success())
}

/// Check if local proxy ports are listening
pub async fn check_local_proxy_ports(socks_port: u16, http_port: u16) -> (bool, bool) {
    let socks_listening = check_port_listening("127.0.0.1", socks_port).await;
    let http_listening = check_port_listening("127.0.0.1", http_port).await;
    (socks_listening, http_listening)
}

async fn check_port_listening(host: &str, port: u16) -> bool {
    use tokio::net::TcpStream;
    
    match timeout(Duration::from_secs(1), TcpStream::connect((host, port))).await {
        Ok(Ok(_)) => true,
        _ => false,
    }
}

/// Check if a port is listening (synchronous version)
pub fn check_port_listening_sync(host: &str, port: u16) -> bool {
    use std::net::TcpStream;
    
    match TcpStream::connect((host, port)) {
        Ok(_) => true,
        Err(_) => false,
    }
}

/// Check if a port is available (not in use) by attempting to bind to it
/// This is a synchronous version that can be called from non-async contexts
pub fn check_port_available_sync(host: &str, port: u16) -> bool {
    use std::net::TcpListener;
    
    match TcpListener::bind((host, port)) {
        Ok(_) => true,  // Successfully bound, port is available
        Err(_) => false,  // Failed to bind, port is in use
    }
}

/// Check if a port is available (not in use) by attempting to bind to it (async version)
pub async fn check_port_available(host: &str, port: u16) -> bool {
    use tokio::net::TcpListener;
    
    match timeout(Duration::from_millis(100), TcpListener::bind((host, port))).await {
        Ok(Ok(_)) => true,  // Successfully bound, port is available
        _ => false,  // Failed to bind, port is in use
    }
}

/// Check if both SOCKS and HTTP ports are available (synchronous version)
/// Checks binding to 0.0.0.0 (all interfaces) since Xray binds to all interfaces by default
pub fn check_ports_available_sync(socks_port: u16, http_port: u16) -> (bool, bool) {
    // Check 0.0.0.0 (all interfaces) since Xray binds to all interfaces by default
    // This is more accurate than checking 127.0.0.1
    let socks_available = check_port_available_sync("0.0.0.0", socks_port);
    let http_available = check_port_available_sync("0.0.0.0", http_port);
    (socks_available, http_available)
}

/// Check if both SOCKS and HTTP ports are available (async version)
pub async fn check_ports_available(socks_port: u16, http_port: u16) -> (bool, bool) {
    let socks_available = check_port_available("127.0.0.1", socks_port).await;
    let http_available = check_port_available("127.0.0.1", http_port).await;
    (socks_available, http_available)
}

/// Find an available port starting from the given port, searching upward
/// Returns the first available port found, or None if no port is available in the range
/// Checks binding to 0.0.0.0 (all interfaces) since Xray binds to all interfaces by default
pub fn find_available_port_sync(start_port: u16, max_attempts: u16) -> Option<u16> {
    for offset in 0..max_attempts {
        let port = start_port.saturating_add(offset);
        if port > 65535 {
            break;
        }
        // Check 0.0.0.0 (all interfaces) since Xray binds to all interfaces by default
        if check_port_available_sync("0.0.0.0", port) {
            return Some(port);
        }
    }
    None
}

/// Find available ports for both SOCKS and HTTP, starting from the given ports
/// Returns (socks_port, http_port) if both are found, or None if not available
pub fn find_available_ports_sync(start_socks: u16, start_http: u16, max_attempts: u16) -> Option<(u16, u16)> {
    // Try to find both ports, ensuring they're different
    let socks_port = find_available_port_sync(start_socks, max_attempts)?;
    
    // For HTTP port, start from start_http but skip if it equals socks_port
    let mut http_start = start_http;
    if http_start == socks_port {
        http_start = http_start.saturating_add(1);
    }
    
    let http_port = find_available_port_sync(http_start, max_attempts)?;
    
    // Make sure they're still different
    if socks_port == http_port {
        // Try next port for HTTP
        if let Some(next_http) = find_available_port_sync(http_port.saturating_add(1), max_attempts) {
            return Some((socks_port, next_http));
        }
        return None;
    }
    
    Some((socks_port, http_port))
}


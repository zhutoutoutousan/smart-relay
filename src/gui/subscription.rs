use anyhow::{Result, Context};
use base64::{engine::general_purpose, Engine as _};
use regex::Regex;
use tracing::{info, warn, debug, error};
use std::time::Duration;
use url::Url;

pub async fn download_subscription(url: &str, use_proxy: bool, socks_port: Option<u16>) -> Result<String> {
    info!("Starting subscription download from: {} (proxy: {}, port: {:?})", url, use_proxy, socks_port);
    
    // Build HTTP client with better configuration (matching v2rayN behavior)
    // v2rayN uses HttpClient which uses HTTP/1.1 by default (not HTTP/2)
    // Our logs show we're using HTTP/2 (ALPN h2), but v2rayN likely uses HTTP/1.1
    // Note: reqwest doesn't have a direct way to disable HTTP/2, but we can try
    // to match v2rayN's behavior by using simpler headers
    let mut client_builder = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(10))
        .tcp_keepalive(Duration::from_secs(60))
        .pool_idle_timeout(Duration::from_secs(90));
    
    // Configure proxy if requested (like v2rayN's blProxy parameter)
    // v2rayN uses socks5://127.0.0.1:{socks_port} for subscription downloads
    if use_proxy {
        let socks_port = socks_port.unwrap_or(1080); // Default to 1080 if not provided
        let proxy_url = format!("socks5://127.0.0.1:{}", socks_port);
        debug!("Configuring subscription download to use proxy: {}", proxy_url);
        client_builder = client_builder.proxy(
            reqwest::Proxy::all(&proxy_url)
                .context("Failed to create SOCKS5 proxy")?
        );
    }
    
    let client = client_builder
        .build()
        .context("Failed to create HTTP client")?;
    
    // Retry logic with exponential backoff
    const MAX_RETRIES: u32 = 3;
    let mut last_error = None;
    
    for attempt in 1..=MAX_RETRIES {
        debug!("Attempt {}/{}: Sending HTTP GET request to {}", attempt, MAX_RETRIES, url);
        
        match try_download(&client, url, 0).await {
            Ok(content) => {
                info!("Download successful on attempt {}", attempt);
                return Ok(content);
            }
            Err(e) => {
                let error_msg = e.to_string();
                error!("Download attempt {} failed: {}", attempt, error_msg);
                last_error = Some(e);
                
                // Check if it's a retryable error
                let error_lower = error_msg.to_lowercase();
                let is_retryable = error_lower.contains("broken pipe")
                    || error_lower.contains("stream closed")
                    || error_lower.contains("connection")
                    || error_lower.contains("timeout")
                    || error_lower.contains("reset")
                    || error_lower.contains("network")
                    || error_lower.contains("tcp")
                    || error_lower.contains("ssl")
                    || error_lower.contains("tls");
                
                if attempt < MAX_RETRIES && is_retryable {
                    let delay = Duration::from_millis(500 * (attempt as u64));
                    warn!("Retrying in {:?}...", delay);
                    tokio::time::sleep(delay).await;
                    continue;
                } else if !is_retryable {
                    // Non-retryable error, return immediately
                    return Err(last_error.unwrap());
                }
            }
        }
    }
    
    // All retries exhausted
    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("Download failed after {} attempts", MAX_RETRIES)))
        .context("Failed to download subscription after retries")
}

async fn try_download(client: &reqwest::Client, url: &str, depth: u32) -> Result<String> {
    // Prevent infinite recursion
    if depth > 2 {
        return Err(anyhow::anyhow!("Maximum redirect depth exceeded (found subscription URL in HTML)"));
    }
    debug!("Sending HTTP GET request to: {}", url);
    
    // Parse URL to extract Basic Auth credentials if present (like v2rayN does)
    // Format: https://user:pass@host/path
    // Some servers check User-Agent and other headers to detect bots
    // Reference: https://github.com/Qv2ray/Qv2ray/issues/509
    let mut request_builder = client.get(url);
    
    // Add standard browser headers to avoid bot detection
    // Some subscription servers return empty content if they detect non-browser requests
    // .NET HttpClient sends minimal headers by default, but we add browser-like ones
    // to match what a real browser would send (v2rayN's HttpClient might add some automatically)
    // NOTE: If using HTTP/2, Connection header is NOT allowed (HTTP/2 spec)
    // If we're using HTTP/2, reqwest will automatically remove invalid headers
    request_builder = request_builder
        .header("Accept", "*/*")  // Simple accept - matches what many HTTP clients send
        .header("Accept-Encoding", "gzip, deflate")  // v2rayN SampleHttpRequest shows this
        .header("Pragma", "no-cache");  // v2rayN SampleHttpRequest shows this
    // Connection header - only add if HTTP/1.1 (HTTP/2 doesn't allow it)
    // We can't easily detect HTTP version before sending, so let reqwest handle it
    
    // Extract Basic Auth from URL if present (v2rayN behavior)
    // Reference: v2rayN DownloadService.cs line 153-156
    if let Ok(parsed_url) = Url::parse(url) {
        if !parsed_url.username().is_empty() || parsed_url.password().is_some() {
            let username = parsed_url.username();
            let password = parsed_url.password().unwrap_or("");
            let auth_string = format!("{}:{}", username, password);
            let encoded = general_purpose::STANDARD.encode(auth_string.as_bytes());
            request_builder = request_builder.header("Authorization", format!("Basic {}", encoded));
            debug!("Added Basic Auth header from URL (user: {})", username);
        }
    }
    
    debug!("Sending request with browser-like headers to avoid bot detection");
    let response = request_builder
        .send()
        .await
        .context("Failed to send HTTP request")?;
    
    let status = response.status();
    info!("Received response with status: {} {}", status.as_u16(), status.canonical_reason().unwrap_or("Unknown"));
    
    // Log all response headers explicitly
    debug!("=== Response Headers ===");
    for (name, value) in response.headers() {
        if let Ok(value_str) = value.to_str() {
            debug!("  {}: {}", name, value_str);
        } else {
            debug!("  {}: <binary>", name);
        }
    }
    debug!("========================");
    
    // Check for redirects that weren't followed
    if status.is_redirection() {
        let location = response.headers().get("location")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        return Err(anyhow::anyhow!(
            "Server returned redirect ({}). Location: {}",
            status.as_u16(),
            location.as_deref().unwrap_or("not provided")
        )).context("Unexpected redirect response");
    }
    
    if !status.is_success() {
        // Try to read error body for more details
        let error_body = response.text().await.unwrap_or_default();
        let error_msg = if !error_body.is_empty() {
            format!("HTTP {} {} - Response: {}", 
                status.as_u16(), 
                status.canonical_reason().unwrap_or("Unknown"),
                error_body.chars().take(200).collect::<String>()
            )
        } else {
            format!("HTTP {} {}", status.as_u16(), status.canonical_reason().unwrap_or("Unknown"))
        };
        
        return Err(anyhow::anyhow!(error_msg))
            .context("Non-success HTTP status code");
    }
    
    // Extract response headers before consuming response body
    let content_encoding = response.headers()
        .get("content-encoding")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let content_length = response.headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<usize>().ok());
    let content_type = response.headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    
    debug!("Response headers - Content-Encoding: {:?}, Content-Length: {:?}, Content-Type: {:?}", 
        content_encoding, content_length, content_type);
    
    // Note: We don't reject Content-Length: 0 here (like v2rayN)
    // Some servers might legitimately return empty responses
    if let Some(len) = content_length {
        if len == 0 {
            debug!("Server indicated Content-Length: 0 - will read empty response");
        }
    }
    
    debug!("Reading response body...");
    
    // First, read raw bytes to see what we're actually getting
    let raw_bytes = response
        .bytes()
        .await
        .context("Failed to read response body")?;
    
    debug!("=== Raw Response Body ===");
    debug!("Raw bytes length: {} bytes", raw_bytes.len());
    if raw_bytes.len() > 0 {
        // Show first 500 bytes as hex and ASCII
        let preview_len = raw_bytes.len().min(500);
        let hex_preview: String = raw_bytes[..preview_len]
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect::<Vec<_>>()
            .chunks(16)
            .enumerate()
            .map(|(i, chunk)| {
                let hex = chunk.join(" ");
                let ascii: String = raw_bytes[i*16..(i*16 + chunk.len()).min(raw_bytes.len())]
                    .iter()
                    .map(|&b| if b >= 32 && b < 127 { b as char } else { '.' })
                    .collect();
                format!("{:04x}: {} |{}|", i * 16, hex, ascii)
            })
            .collect::<Vec<_>>()
            .join("\n");
        debug!("First {} bytes (hex + ASCII):\n{}", preview_len, hex_preview);
        
        // Try to detect if it's compressed
        if raw_bytes.len() >= 2 {
            let magic = &raw_bytes[..2];
            if magic == b"\x1f\x8b" {
                debug!("Detected gzip magic bytes (1f 8b)");
            } else if magic == b"\x78\x9c" || magic == b"\x78\x01" || magic == b"\x78\xda" {
                debug!("Detected zlib/deflate magic bytes");
            } else {
                debug!("No compression magic bytes detected");
            }
        }
    } else {
        debug!("Response body is completely empty (0 bytes)");
    }
    debug!("=========================");
    
    // Now try to decode as text (reqwest should handle decompression automatically)
    // But since we already consumed the response with .bytes(), we need to recreate it
    // Actually, we can't - once we call .bytes(), the response is consumed
    // So let's manually handle decompression if needed
    let content = if raw_bytes.is_empty() {
        String::new()
    } else {
        // Check Content-Encoding header to determine if we need to decompress
        if let Some(encoding) = &content_encoding {
            if encoding.contains("gzip") {
                use flate2::read::GzDecoder;
                use std::io::Read;
                debug!("Attempting gzip decompression...");
                let mut decoder = GzDecoder::new(&raw_bytes[..]);
                let mut decompressed = String::new();
                match decoder.read_to_string(&mut decompressed) {
                    Ok(_) => {
                        debug!("Gzip decompression successful: {} bytes -> {} bytes", raw_bytes.len(), decompressed.len());
                        decompressed
                    }
                    Err(e) => {
                        warn!("Gzip decompression failed: {}, trying as plain text", e);
                        String::from_utf8_lossy(&raw_bytes).to_string()
                    }
                }
            } else if encoding.contains("deflate") {
                use flate2::read::DeflateDecoder;
                use std::io::Read;
                debug!("Attempting deflate decompression...");
                let mut decoder = DeflateDecoder::new(&raw_bytes[..]);
                let mut decompressed = String::new();
                match decoder.read_to_string(&mut decompressed) {
                    Ok(_) => {
                        debug!("Deflate decompression successful: {} bytes -> {} bytes", raw_bytes.len(), decompressed.len());
                        decompressed
                    }
                    Err(e) => {
                        warn!("Deflate decompression failed: {}, trying as plain text", e);
                        String::from_utf8_lossy(&raw_bytes).to_string()
                    }
                }
            } else {
                // No compression or unknown encoding
                debug!("No decompression needed, decoding as UTF-8");
                String::from_utf8_lossy(&raw_bytes).to_string()
            }
        } else {
            // No Content-Encoding header - but check for magic bytes anyway
            if raw_bytes.len() >= 2 && &raw_bytes[..2] == b"\x1f\x8b" {
                // Gzip magic bytes but no Content-Encoding header - decompress anyway
                use flate2::read::GzDecoder;
                use std::io::Read;
                debug!("Detected gzip magic bytes, attempting decompression despite missing Content-Encoding header...");
                let mut decoder = GzDecoder::new(&raw_bytes[..]);
                let mut decompressed = String::new();
                match decoder.read_to_string(&mut decompressed) {
                    Ok(_) => {
                        debug!("Gzip decompression successful: {} bytes -> {} bytes", raw_bytes.len(), decompressed.len());
                        decompressed
                    }
                    Err(e) => {
                        warn!("Gzip decompression failed: {}, trying as plain text", e);
                        String::from_utf8_lossy(&raw_bytes).to_string()
                    }
                }
            } else {
                // No compression detected, decode as UTF-8
                debug!("No compression detected, decoding as UTF-8");
                String::from_utf8_lossy(&raw_bytes).to_string()
            }
        }
    };
    
    debug!("=== Decoded Content ===");
    debug!("Content length: {} bytes", content.len());
    if content.len() > 0 {
        let preview = content.chars().take(500).collect::<String>();
        debug!("First 500 chars: {}", preview);
    } else {
        debug!("Content is empty");
    }
    debug!("======================");
    
    // Check if response is HTML (like v2rayN, we still try to process it)
    // Subscription content might be embedded in HTML (base64, JSON, etc.)
    let is_html = content_type.as_deref()
        .map(|ct| ct.contains("text/html"))
        .unwrap_or(false) || content.trim_start().starts_with("<!DOCTYPE") || content.trim_start().starts_with("<html");
    
    if is_html && !content.is_empty() {
        debug!("Server returned HTML response ({} bytes), attempting to extract subscription content", content.len());
        
        // Try to extract subscription content from HTML
        // Some servers embed base64 subscription data in HTML
        // Look for common patterns: base64 strings, JSON, or subscription URLs
        let trimmed = content.trim();
        
        // Check if HTML contains base64-encoded subscription (common pattern)
        if let Some(base64_match) = extract_base64_from_html(&trimmed) {
            info!("Found base64 subscription data in HTML response");
            return Ok(base64_match);
        }
        
        // Check if HTML contains a subscription URL or link
        if let Some(sub_url) = extract_subscription_url_from_html(&trimmed) {
            info!("Found subscription URL in HTML, downloading from: {}", sub_url);
            // Recursively download from the found URL (with depth limit)
            return Box::pin(try_download(client, &sub_url, depth + 1)).await;
        }
        
        // If HTML contains error indicators, warn but still try to parse
        if trimmed.contains("error") || trimmed.contains("Error") || trimmed.contains("403") || trimmed.contains("401") {
            warn!("HTML response contains error indicators, but attempting to parse anyway");
        }
        
        // v2rayN would still try to parse this - maybe it's valid subscription data
        // that just happens to be served as HTML. Continue processing.
    }
    
    // Like v2rayN, we don't reject empty content immediately
    // The parser will handle it and return appropriate errors if needed
    // Some servers might return empty responses that are valid (no endpoints available)
    if content.is_empty() {
        warn!("Server returned empty content, but continuing (like v2rayN behavior)");
        // Return empty string - let the parser decide if this is an error
        // This matches v2rayN's behavior where it returns the content as-is
        return Ok(String::new());
    }
    
    // Check if content is just whitespace - still return it (v2rayN behavior)
    let trimmed = content.trim();
    if trimmed.is_empty() {
        warn!("Response contains only whitespace, but continuing");
        return Ok(String::new());
    }
    
    info!("Downloaded {} bytes of content", content.len());
    debug!("Content preview (first 200 chars): {}", &content.chars().take(200).collect::<String>());
    
    // Check if content looks like an error message
    let content_lower = content.to_lowercase();
    if content_lower.contains("error") || content_lower.contains("unauthorized") || 
       content_lower.contains("forbidden") || content_lower.contains("invalid") ||
       content_lower.contains("expired") || content_lower.contains("not found") {
        warn!("Response content may indicate an error: {}", &content.chars().take(100).collect::<String>());
    }
    
    Ok(content)
}

pub fn parse_subscription_content(content: &str) -> Result<Vec<ParsedEndpoint>> {
    info!("Parsing subscription content ({} bytes)", content.len());
    
    // Handle empty content gracefully (like v2rayN does)
    // Empty content might mean no endpoints available, not necessarily an error
    if content.trim().is_empty() {
        warn!("Subscription content is empty - returning empty endpoint list");
        return Ok(Vec::new());
    }
    
    let mut endpoints = Vec::new();
    
    // Try to decode as base64 first
    debug!("Attempting base64 decode...");
    let decoded = if let Ok(decoded) = general_purpose::STANDARD.decode(content.trim()) {
        debug!("Base64 decode successful, decoded {} bytes", decoded.len());
        match String::from_utf8(decoded) {
            Ok(s) => {
                info!("Decoded to UTF-8 string ({} chars)", s.len());
                s
            }
            Err(e) => {
                warn!("Base64 decoded but UTF-8 conversion failed: {}", e);
                content.to_string()
            }
        }
    } else {
        debug!("Not base64 encoded, using content as-is");
        content.to_string()
    };
    
    let lines: Vec<&str> = decoded.lines().collect();
    info!("Found {} lines to parse", lines.len());
    
    // Split by lines and parse each
    for (idx, line) in lines.iter().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        
        debug!("Parsing line {}: {}...", idx + 1, &line.chars().take(50).collect::<String>());
        if let Some(endpoint) = parse_config_line(line) {
            info!("Successfully parsed endpoint: {} ({})", endpoint.name, endpoint.protocol);
            endpoints.push(endpoint);
        } else {
            debug!("Failed to parse line {} as any known protocol", idx + 1);
        }
    }
    
    info!("Parsed {} endpoints total", endpoints.len());
    Ok(endpoints)
}

fn parse_config_line(line: &str) -> Option<ParsedEndpoint> {
    // VMess: vmess://base64
    if line.starts_with("vmess://") {
        return parse_vmess(line);
    }
    
    // VLESS: vless://...
    if line.starts_with("vless://") {
        return parse_vless(line);
    }
    
    // Shadowsocks: ss://base64 or ss://method:password@host:port#remark
    if line.starts_with("ss://") {
        return parse_shadowsocks(line);
    }
    
    // Trojan: trojan://password@host:port?...
    if line.starts_with("trojan://") {
        return parse_trojan(line);
    }
    
    // SOCKS5: socks://...
    if line.starts_with("socks://") || line.starts_with("socks5://") {
        return parse_socks5(line);
    }
    
    None
}

fn parse_vmess(url: &str) -> Option<ParsedEndpoint> {
    // vmess://base64
    let base64_part = url.strip_prefix("vmess://")?;
    let decoded = match general_purpose::STANDARD.decode(base64_part) {
        Ok(d) => d,
        Err(e) => {
            debug!("VMess base64 decode failed: {}", e);
            return None;
        }
    };
    
    let json_str = match String::from_utf8(decoded) {
        Ok(s) => s,
        Err(e) => {
            debug!("VMess UTF-8 decode failed: {}", e);
            return None;
        }
    };
    
    let json: serde_json::Value = match serde_json::from_str(&json_str) {
        Ok(j) => j,
        Err(e) => {
            debug!("VMess JSON parse failed: {}", e);
            return None;
        }
    };
    
    // Try different field names for name/remark
    let name = json.get("ps")
        .or_else(|| json.get("name"))
        .or_else(|| json.get("remark"))
        .and_then(|v| v.as_str())
        .unwrap_or("VMess Server")
        .to_string();
    
    let host = json.get("add")
        .or_else(|| json.get("address"))
        .or_else(|| json.get("host"))
        .and_then(|v| v.as_str())?;
    
    let port = json.get("port")
        .and_then(|v| v.as_u64())
        .or_else(|| json.get("port").and_then(|v| v.as_str()).and_then(|s| s.parse::<u64>().ok()))?;
    
    Some(ParsedEndpoint {
        name,
        host: host.to_string(),
        port: port as u16,
        protocol: "vmess".to_string(),
        tags: vec![],
        raw_config: Some(json.clone()),
    })
}

fn parse_vless(url: &str) -> Option<ParsedEndpoint> {
    // vless://uuid@host:port?type=tcp&security=none#remark
    let re = Regex::new(r"vless://([^@]+)@([^:]+):(\d+)(\?[^#]*)?(?:#(.+))?").ok()?;
    let caps = re.captures(url)?;
    
    Some(ParsedEndpoint {
        name: caps.get(5).map(|m| m.as_str().to_string())
            .unwrap_or_else(|| "VLESS Server".to_string()),
        host: caps.get(2)?.as_str().to_string(),
        port: caps.get(3)?.as_str().parse().ok()?,
        protocol: "vless".to_string(),
        tags: vec![],
        raw_config: None,
    })
}

fn parse_shadowsocks(url: &str) -> Option<ParsedEndpoint> {
    // ss://base64 or ss://method:password@host:port#remark
    if let Some(base64_part) = url.strip_prefix("ss://") {
        // Try base64 first
        if let Ok(decoded) = general_purpose::STANDARD.decode(base64_part.split('#').next()?) {
            if let Ok(decoded_str) = String::from_utf8(decoded) {
                // Format: method:password@host:port
                let parts: Vec<&str> = decoded_str.split('@').collect();
                if parts.len() == 2 {
                    let method_pass: Vec<&str> = parts[0].split(':').collect();
                    let host_port: Vec<&str> = parts[1].rsplitn(2, ':').collect();
                    if method_pass.len() == 2 && host_port.len() == 2 {
                        let method = method_pass[0];
                        let password = method_pass[1];
                        let host = host_port[1];
                        let port = host_port[0].parse().ok()?;
                        let name = url.split('#').nth(1).unwrap_or("Shadowsocks").to_string();
                        
                        // Store method and password in raw_config
                        let raw_config = serde_json::json!({
                            "method": method,
                            "password": password
                        });
                        
                        return Some(ParsedEndpoint {
                            name,
                            host: host.to_string(),
                            port,
                            protocol: "shadowsocks".to_string(),
                            tags: vec![],
                            raw_config: Some(raw_config),
                        });
                    }
                }
            }
        }
        
        // Try URL format: ss://method:password@host:port#remark
        let re = Regex::new(r"ss://([^@]+)@([^:]+):(\d+)(?:#(.+))?").ok()?;
        if let Some(caps) = re.captures(url) {
            let method_pass = caps.get(1)?.as_str();
            let method_pass_parts: Vec<&str> = method_pass.split(':').collect();
            if method_pass_parts.len() == 2 {
                let method = method_pass_parts[0];
                let password = method_pass_parts[1];
                let host = caps.get(2)?.as_str();
                let port = caps.get(3)?.as_str().parse().ok()?;
                let name = caps.get(4).map(|m| m.as_str().to_string())
                    .unwrap_or_else(|| "Shadowsocks".to_string());
                
                // Store method and password in raw_config
                let raw_config = serde_json::json!({
                    "method": method,
                    "password": password
                });
                
                return Some(ParsedEndpoint {
                    name,
                    host: host.to_string(),
                    port,
                    protocol: "shadowsocks".to_string(),
                    tags: vec![],
                    raw_config: Some(raw_config),
                });
            }
        }
    }
    None
}

fn parse_trojan(url: &str) -> Option<ParsedEndpoint> {
    // trojan://password@host:port?type=tcp#remark
    let re = Regex::new(r"trojan://([^@]+)@([^:]+):(\d+)(\?[^#]*)?(?:#(.+))?").ok()?;
    let caps = re.captures(url)?;
    
    let password = caps.get(1)?.as_str();
    let host = caps.get(2)?.as_str();
    let port = caps.get(3)?.as_str().parse().ok()?;
    let name = caps.get(5).map(|m| m.as_str().to_string())
        .unwrap_or_else(|| "Trojan Server".to_string());
    
    // Store password in raw_config
    let raw_config = serde_json::json!({
        "password": password
    });
    
    Some(ParsedEndpoint {
        name,
        host: host.to_string(),
        port,
        protocol: "trojan".to_string(),
        tags: vec![],
        raw_config: Some(raw_config),
    })
}

fn parse_socks5(url: &str) -> Option<ParsedEndpoint> {
    // socks://user:pass@host:port or socks5://...
    let re = Regex::new(r"socks5?://([^@]+@)?([^:]+):(\d+)(?:#(.+))?").ok()?;
    let caps = re.captures(url)?;
    
    Some(ParsedEndpoint {
        name: caps.get(4).map(|m| m.as_str().to_string())
            .unwrap_or_else(|| "SOCKS5 Server".to_string()),
        host: caps.get(2)?.as_str().to_string(),
        port: caps.get(3)?.as_str().parse().ok()?,
        protocol: "socks5".to_string(),
        tags: vec![],
        raw_config: None,
    })
}

/// Extract base64-encoded subscription data from HTML
/// Some subscription servers embed the data in HTML pages (like v2rayN handles)
fn extract_base64_from_html(html: &str) -> Option<String> {
    // Look for base64 patterns in HTML (common in subscription pages)
    // Pattern: long base64 strings that might be subscription data
    let base64_pattern = Regex::new(r"([A-Za-z0-9+/]{100,}={0,2})").ok()?;
    
    for cap in base64_pattern.captures_iter(html) {
        if let Some(matched) = cap.get(1) {
            let candidate = matched.as_str();
            // Try to decode to see if it's valid base64 subscription data
            if let Ok(decoded) = general_purpose::STANDARD.decode(candidate) {
                if let Ok(decoded_str) = String::from_utf8(decoded) {
                    // Check if decoded content looks like subscription data
                    if decoded_str.contains("vmess://") || decoded_str.contains("vless://") || 
                       decoded_str.contains("ss://") || decoded_str.contains("trojan://") {
                        return Some(candidate.to_string());
                    }
                }
            }
        }
    }
    None
}

/// Extract subscription URL from HTML (e.g., in meta tags, links, or JavaScript)
/// Some servers return HTML pages that contain links to the actual subscription
fn extract_subscription_url_from_html(html: &str) -> Option<String> {
    // Look for subscription URLs in various HTML patterns
    let patterns = vec![
        r#"href=["']([^"']*(?:subscribe|sub|proxy|config)[^"']*)["']"#,
        r#"url["']?\s*[:=]\s*["']([^"']*(?:subscribe|sub|proxy|config)[^"']*)["']"#,
        r#"<meta[^>]*content=["']([^"']*(?:subscribe|sub|proxy|config)[^"']*)["']"#,
    ];
    
    for pattern_str in patterns {
        if let Ok(re) = Regex::new(pattern_str) {
            if let Some(cap) = re.captures(html) {
                if let Some(url_match) = cap.get(1) {
                    let found_url = url_match.as_str();
                    // Validate it looks like a URL
                    if found_url.starts_with("http://") || found_url.starts_with("https://") {
                        return Some(found_url.to_string());
                    }
                }
            }
        }
    }
    None
}

#[derive(Debug, Clone)]
pub struct ParsedEndpoint {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub protocol: String,
    pub tags: Vec<String>,
    pub raw_config: Option<serde_json::Value>, // Full config for protocols like VMess
}


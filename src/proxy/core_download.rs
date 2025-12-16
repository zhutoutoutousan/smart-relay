use anyhow::{Context, Result};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::fs;
use tracing::info;
use directories::ProjectDirs;

#[cfg(target_os = "windows")]
const XRAY_BINARY_NAME: &str = "xray.exe";
#[cfg(not(target_os = "windows"))]
const XRAY_BINARY_NAME: &str = "xray";

#[cfg(target_os = "windows")]
const SING_BOX_BINARY_NAME: &str = "sing-box.exe";
#[cfg(not(target_os = "windows"))]
const SING_BOX_BINARY_NAME: &str = "sing-box";

/// Get the platform-specific download URL for Xray
fn get_xray_download_url(version: &str) -> Result<String> {
    let arch = if cfg!(target_arch = "aarch64") {
        "arm64-v8a"
    } else {
        "64"
    };
    
    let os = if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    };
    
    let ext = if cfg!(target_os = "windows") { "zip" } else { "zip" };
    let url = format!(
        "https://github.com/XTLS/Xray-core/releases/download/v{}/Xray-{}-{}.{}",
        version, os, arch, ext
    );
    
    Ok(url)
}

/// Get the platform-specific download URL for sing-box
fn get_sing_box_download_url(version: &str) -> Result<String> {
    let arch = if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "amd64"
    };
    
    let os = if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "darwin"
    } else {
        "linux"
    };
    
    let ext = if cfg!(target_os = "windows") { "zip" } else { "tar.gz" };
    let url = format!(
        "https://github.com/SagerNet/sing-box/releases/download/v{}/sing-box-{}-{}-{}.{}",
        version, version, os, arch, ext
    );
    
    Ok(url)
}

/// Get latest version from GitHub API
async fn get_latest_version(repo: &str) -> Result<String> {
    let api_url = format!("https://api.github.com/repos/{}/releases/latest", repo);
    info!("Fetching latest version from: {}", api_url);
    
    let client = reqwest::Client::builder()
        .user_agent("Smart-Relay/1.0")
        .timeout(std::time::Duration::from_secs(30))
        .build()?;
    
    let response = client.get(&api_url).send().await?;
    let json: Value = response.json().await?;
    
    let tag_name = json.get("tag_name")
        .and_then(|v| v.as_str())
        .context("No tag_name in response")?;
    
    // Remove 'v' prefix if present
    let version = tag_name.strip_prefix('v').unwrap_or(tag_name);
    info!("Latest version: {}", version);
    
    Ok(version.to_string())
}

/// Get the directory where cores should be stored
fn get_cores_dir() -> Result<PathBuf> {
    let dirs = ProjectDirs::from("com", "smart-relay", "smart-relay")
        .context("Failed to get project directories")?;
    
    let cores_dir = dirs.data_local_dir().join("cores");
    fs::create_dir_all(&cores_dir)?;
    
    Ok(cores_dir)
}

/// Download and extract Xray core
pub async fn download_xray(version: Option<String>) -> Result<PathBuf> {
    info!("Starting Xray download...");
    
    let version = match version {
        Some(v) => v,
        None => get_latest_version("XTLS/Xray-core").await?,
    };
    
    let url = get_xray_download_url(&version)?;
    info!("Downloading Xray from: {}", url);
    
    let cores_dir = get_cores_dir()?;
    let xray_dir = cores_dir.join("xray");
    fs::create_dir_all(&xray_dir)?;
    
    let xray_binary_path = xray_dir.join(XRAY_BINARY_NAME);
    
    // Check if already downloaded
    if xray_binary_path.exists() {
        info!("Xray binary already exists at: {:?}", xray_binary_path);
        return Ok(xray_binary_path);
    }
    
    // Download
    info!("Downloading from: {}", url);
    let client = reqwest::Client::builder()
        .user_agent("Smart-Relay/1.0")
        .timeout(std::time::Duration::from_secs(300))
        .build()?;
    
    info!("Sending download request...");
    let response = client.get(&url).send().await?;
    info!("Downloading file (size: {} bytes)...", response.content_length().unwrap_or(0));
    let bytes = response.bytes().await?;
    info!("Downloaded {} bytes, extracting...", bytes.len());
    
    // Extract
    let temp_dir = tempfile::tempdir()?;
    let zip_path = temp_dir.path().join("xray.zip");
    fs::write(&zip_path, bytes)?;
    
    #[cfg(target_os = "windows")]
    {
        use std::process::Command;
        // Use PowerShell to extract on Windows
        let output = Command::new("powershell")
            .args(&["-Command", &format!("Expand-Archive -Path '{}' -DestinationPath '{}' -Force", zip_path.display(), temp_dir.path().display())])
            .output()?;
        
        if !output.status.success() {
            return Err(anyhow::anyhow!("Failed to extract zip: {}", String::from_utf8_lossy(&output.stderr)));
        }
    }
    
    #[cfg(not(target_os = "windows"))]
    {
        // Use zip crate or system unzip
        let file = std::fs::File::open(&zip_path)?;
        let mut archive = zip::ZipArchive::new(std::io::BufReader::new(file))?;
        
        for i in 0..archive.len() {
            let mut file = archive.by_index(i)?;
            let outpath = temp_dir.path().join(file.mangled_name());
            
            if file.name().ends_with('/') {
                fs::create_dir_all(&outpath)?;
            } else {
                if let Some(p) = outpath.parent() {
                    fs::create_dir_all(p)?;
                }
                let mut outfile = fs::File::create(&outpath)?;
                std::io::copy(&mut file, &mut outfile)?;
            }
        }
    }
    
    // Find and copy xray binary
    let xray_binary = find_binary_in_dir(temp_dir.path(), "xray")?;
    fs::copy(&xray_binary, &xray_binary_path)?;
    
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&xray_binary_path, fs::Permissions::from_mode(0o755))?;
    }
    
    info!("Xray downloaded successfully to: {:?}", xray_binary_path);
    
    // Download geoip.dat and geosite.dat
    info!("Downloading geoip.dat and geosite.dat...");
    download_xray_geo_files(&xray_dir).await?;
    
    Ok(xray_binary_path)
}

/// Download Xray geo files (geoip.dat and geosite.dat)
pub async fn download_xray_geo_files(xray_dir: &PathBuf) -> Result<()> {
    use std::fs;
    
    let geoip_path = xray_dir.join("geoip.dat");
    let geosite_path = xray_dir.join("geosite.dat");
    
    // Check if files already exist
    if geoip_path.exists() && geosite_path.exists() {
        info!("Geo files already exist, skipping download");
        return Ok(());
    }
    
    let client = reqwest::Client::builder()
        .user_agent("Smart-Relay/1.0")
        .timeout(std::time::Duration::from_secs(300))
        .build()?;
    
    // Download geoip.dat
    if !geoip_path.exists() {
        info!("Downloading geoip.dat...");
        let url = "https://github.com/Loyalsoldier/v2ray-rules-dat/releases/latest/download/geoip.dat";
        let response = client.get(url).send().await?;
        let bytes = response.bytes().await?;
        fs::write(&geoip_path, bytes)?;
        info!("geoip.dat downloaded to: {:?}", geoip_path);
    }
    
    // Download geosite.dat
    if !geosite_path.exists() {
        info!("Downloading geosite.dat...");
        let url = "https://github.com/Loyalsoldier/v2ray-rules-dat/releases/latest/download/geosite.dat";
        let response = client.get(url).send().await?;
        let bytes = response.bytes().await?;
        fs::write(&geosite_path, bytes)?;
        info!("geosite.dat downloaded to: {:?}", geosite_path);
    }
    
    Ok(())
}

/// Download and extract sing-box core
pub async fn download_sing_box(version: Option<String>) -> Result<PathBuf> {
    info!("Starting sing-box download...");
    
    let version = match version {
        Some(v) => v,
        None => get_latest_version("SagerNet/sing-box").await?,
    };
    
    let url = get_sing_box_download_url(&version)?;
    info!("Downloading sing-box from: {}", url);
    
    let cores_dir = get_cores_dir()?;
    let sing_box_dir = cores_dir.join("sing-box");
    fs::create_dir_all(&sing_box_dir)?;
    
    let sing_box_binary_path = sing_box_dir.join(SING_BOX_BINARY_NAME);
    
    // Check if already downloaded
    if sing_box_binary_path.exists() {
        info!("sing-box binary already exists at: {:?}", sing_box_binary_path);
        return Ok(sing_box_binary_path);
    }
    
    // Download
    info!("Downloading from: {}", url);
    let client = reqwest::Client::builder()
        .user_agent("Smart-Relay/1.0")
        .timeout(std::time::Duration::from_secs(300))
        .build()?;
    
    info!("Sending download request...");
    let response = client.get(&url).send().await?;
    info!("Downloading file (size: {} bytes)...", response.content_length().unwrap_or(0));
    let bytes = response.bytes().await?;
    info!("Downloaded {} bytes, extracting...", bytes.len());
    
    // Extract
    let temp_dir = tempfile::tempdir()?;
    
    if url.ends_with(".zip") {
        let zip_path = temp_dir.path().join("sing-box.zip");
        fs::write(&zip_path, bytes)?;
        
        // Extract zip using zip crate
        let file = std::fs::File::open(&zip_path)?;
        let mut archive = zip::ZipArchive::new(std::io::BufReader::new(file))?;
        
        for i in 0..archive.len() {
            let mut file = archive.by_index(i)?;
            let outpath = temp_dir.path().join(file.mangled_name());
            
            if file.name().ends_with('/') {
                fs::create_dir_all(&outpath)?;
            } else {
                if let Some(p) = outpath.parent() {
                    fs::create_dir_all(p)?;
                }
                let mut outfile = fs::File::create(&outpath)?;
                std::io::copy(&mut file, &mut outfile)?;
            }
        }
    } else {
        // tar.gz
        let tar_path = temp_dir.path().join("sing-box.tar.gz");
        fs::write(&tar_path, bytes)?;
        
        // Extract tar.gz using system tar command
        use std::process::Command;
        let output = Command::new("tar")
            .args(&["-xzf", tar_path.to_str().unwrap(), "-C", temp_dir.path().to_str().unwrap()])
            .output();
        
        match output {
            Ok(output) if output.status.success() => {}
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(anyhow::anyhow!("Failed to extract tar.gz: {}", stderr));
            }
            Err(e) => {
                return Err(anyhow::anyhow!("Failed to run tar command: {}. Please ensure tar is installed", e));
            }
        }
    }
    
    // Find and copy sing-box binary
    let sing_box_binary = find_binary_in_dir(temp_dir.path(), "sing-box")?;
    fs::copy(&sing_box_binary, &sing_box_binary_path)?;
    
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&sing_box_binary_path, fs::Permissions::from_mode(0o755))?;
    }
    
    info!("sing-box downloaded successfully to: {:?}", sing_box_binary_path);
    Ok(sing_box_binary_path)
}

/// Find binary file in directory (recursive)
fn find_binary_in_dir(dir: &Path, name: &str) -> Result<PathBuf> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        
        if path.is_file() {
            let file_name = path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("");
            
            if file_name == name || 
               file_name == format!("{}.exe", name) ||
               file_name.starts_with(name) {
                return Ok(path);
            }
        } else if path.is_dir() {
            if let Ok(found) = find_binary_in_dir(&path, name) {
                return Ok(found);
            }
        }
    }
    
    Err(anyhow::anyhow!("Binary '{}' not found in extracted archive", name))
}

/// Check if core binary exists
pub fn check_core_exists(core_type: &str) -> bool {
    let cores_dir = match get_cores_dir() {
        Ok(d) => d,
        Err(_) => return false,
    };
    
    let binary_name = match core_type {
        "xray" => XRAY_BINARY_NAME,
        "sing-box" => SING_BOX_BINARY_NAME,
        _ => return false,
    };
    
    let binary_path = cores_dir.join(core_type).join(binary_name);
    binary_path.exists()
}

/// Get path to core binary if it exists
pub fn get_core_path(core_type: &str) -> Option<PathBuf> {
    let cores_dir = get_cores_dir().ok()?;
    
    let binary_name = match core_type {
        "xray" => XRAY_BINARY_NAME,
        "sing-box" => SING_BOX_BINARY_NAME,
        _ => return None,
    };
    
    let binary_path = cores_dir.join(core_type).join(binary_name);
    if binary_path.exists() {
        Some(binary_path)
    } else {
        None
    }
}


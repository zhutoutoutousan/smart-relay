use anyhow::{anyhow, Result};
#[allow(unused_imports)]
use tracing::{info, warn};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProxyMode {
    Direct,      // No proxy
    Global,      // Global proxy (all traffic)
    Pac,         // PAC mode (auto-detect)
    Unchanged,   // Don't change system settings
}

impl Default for ProxyMode {
    fn default() -> Self {
        ProxyMode::Unchanged
    }
}

#[derive(Debug, Clone)]
pub struct ProxyConfig {
    pub mode: ProxyMode,
    pub socks_port: u16,
    pub http_port: u16,
    pub pac_url: Option<String>,
    pub exceptions: Vec<String>, // Domains/IPs to bypass
    pub bypass_local: bool,
}

impl Default for ProxyConfig {
    fn default() -> Self {
        Self {
            mode: ProxyMode::Unchanged,
            socks_port: 1080,
            http_port: 8888, // Changed from 8080 to avoid conflict with Docker Desktop on Windows
            pac_url: None,
            exceptions: vec![],
            bypass_local: true,
        }
    }
}

pub struct WindowsProxyManager;

impl WindowsProxyManager {
    /// Set Windows system proxy
    pub fn set_proxy(config: &ProxyConfig) -> Result<()> {
        match config.mode {
            ProxyMode::Direct => Self::unset_proxy(),
            ProxyMode::Global => Self::set_global_proxy(config),
            ProxyMode::Pac => Self::set_pac_proxy(config),
            ProxyMode::Unchanged => Ok(()),
        }
    }
    
    fn set_global_proxy(config: &ProxyConfig) -> Result<()> {
        info!("Setting Windows global proxy: {}:{}", "127.0.0.1", config.socks_port);
        
        #[cfg(windows)]
        {
            use winreg::enums::*;
            use winreg::RegKey;
            
            let proxy_str = format!("127.0.0.1:{}", config.socks_port);
            let exceptions = if config.bypass_local {
                format!("<local>;{}", config.exceptions.join(";"))
            } else {
                config.exceptions.join(";")
            };
            
            let hkcu = RegKey::predef(HKEY_CURRENT_USER);
            let internet_settings = hkcu.open_subkey_with_flags(
                r"Software\Microsoft\Windows\CurrentVersion\Internet Settings",
                KEY_WRITE,
            )?;
            
            // Enable proxy
            internet_settings.set_value("ProxyEnable", &1u32)?;
            internet_settings.set_value("ProxyServer", &proxy_str)?;
            internet_settings.set_value("ProxyOverride", &exceptions)?;
            let empty: String = String::new();
            internet_settings.set_value("AutoConfigURL", &empty)?;
            
            info!("Windows proxy set successfully via registry");
        }
        
        #[cfg(not(windows))]
        {
            return Err(anyhow!("Windows proxy setting not available on this platform"));
        }
        
        Ok(())
    }
    
    fn set_pac_proxy(config: &ProxyConfig) -> Result<()> {
        let pac_url = config.pac_url.as_ref()
            .ok_or_else(|| anyhow!("PAC URL required for PAC mode"))?;
        
        info!("Setting Windows PAC proxy: {}", pac_url);
        
        #[cfg(windows)]
        {
            use winreg::enums::*;
            use winreg::RegKey;
            
            let hkcu = RegKey::predef(HKEY_CURRENT_USER);
            let internet_settings = hkcu.open_subkey_with_flags(
                r"Software\Microsoft\Windows\CurrentVersion\Internet Settings",
                KEY_WRITE,
            )?;
            
            // Disable manual proxy, enable PAC
            internet_settings.set_value("ProxyEnable", &0u32)?;
            let empty: String = String::new();
            internet_settings.set_value("ProxyServer", &empty)?;
            internet_settings.set_value("ProxyOverride", &empty)?;
            internet_settings.set_value("AutoConfigURL", pac_url)?;
            
            info!("Windows PAC proxy set successfully via registry");
        }
        
        #[cfg(not(windows))]
        {
            return Err(anyhow!("Windows proxy setting not available on this platform"));
        }
        
        Ok(())
    }
    
    pub fn unset_proxy() -> Result<()> {
        info!("Unsetting Windows proxy");
        
        #[cfg(windows)]
        {
            use winreg::enums::*;
            use winreg::RegKey;
            
            let hkcu = RegKey::predef(HKEY_CURRENT_USER);
            let internet_settings = hkcu.open_subkey_with_flags(
                r"Software\Microsoft\Windows\CurrentVersion\Internet Settings",
                KEY_WRITE,
            )?;
            
            internet_settings.set_value("ProxyEnable", &0u32)?;
            let empty: String = String::new();
            internet_settings.set_value("ProxyServer", &empty)?;
            internet_settings.set_value("ProxyOverride", &empty)?;
            internet_settings.set_value("AutoConfigURL", &empty)?;
            
            info!("Windows proxy unset successfully via registry");
        }
        
        #[cfg(not(windows))]
        {
            warn!("Proxy unset not implemented for this platform");
        }
        
        Ok(())
    }
    
    pub fn get_current_proxy() -> Result<Option<String>> {
        #[cfg(windows)]
        {
            use winreg::enums::*;
            use winreg::RegKey;
            
            let hkcu = RegKey::predef(HKEY_CURRENT_USER);
            let internet_settings = hkcu.open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Internet Settings")?;
            
            let enabled: u32 = internet_settings.get_value("ProxyEnable").unwrap_or(0);
            if enabled == 1 {
                let proxy: String = internet_settings.get_value("ProxyServer").unwrap_or_default();
                if !proxy.is_empty() {
                    return Ok(Some(proxy));
                }
            }
        }
        
        Ok(None)
    }
}

#[cfg(target_os = "windows")]
pub fn set_system_proxy(config: &ProxyConfig) -> Result<()> {
    WindowsProxyManager::set_proxy(config)
}

#[cfg(not(target_os = "windows"))]
pub fn set_system_proxy(_config: &ProxyConfig) -> Result<()> {
    warn!("System proxy setting not implemented for this platform");
    Ok(())
}


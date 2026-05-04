//! On-disk config (TOML). The GUI reads/writes this; the daemon watches it.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// `server` owns the active mouse/keyboard; `client` receives forwarded input.
    pub role: Role,
    pub bind_addr: String,
    pub server_addr: Option<String>,
    pub layout: crate::layout::Layout,
    #[serde(default)]
    pub clipboard_sync: bool,
    #[serde(default)]
    pub tls: Option<TlsConfig>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Server,
    Client,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TlsConfig {
    pub cert: PathBuf,
    pub key: PathBuf,
    pub trust: PathBuf,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            role: Role::Server,
            bind_addr: format!("0.0.0.0:{}", dshare_protocol::DEFAULT_PORT),
            server_addr: None,
            layout: crate::layout::Layout::default(),
            clipboard_sync: true,
            tls: None,
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let raw = std::fs::read_to_string(path)?;
        Ok(toml::from_str(&raw)?)
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, toml::to_string_pretty(self)?)?;
        Ok(())
    }

    pub fn default_path() -> PathBuf {
        dirs_path().join("config.toml")
    }
}

fn dirs_path() -> PathBuf {
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".config").join("dshare");
    }
    if let Some(appdata) = std::env::var_os("APPDATA") {
        return PathBuf::from(appdata).join("dshare");
    }
    PathBuf::from(".")
}

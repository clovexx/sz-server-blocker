use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

use crate::firewall::Backend;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum BackendPref {
    #[default]
    Auto,
    Nftables,
    Iptables,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub blocked: Vec<String>,
    #[serde(default)]
    pub backend: BackendPref,
    #[serde(default)]
    pub apply_on_boot: bool,
    #[serde(default = "default_sync_on_startup")]
    pub sync_on_startup: bool,
    #[serde(default = "default_api_login")]
    pub api_login: String,
}

fn default_api_login() -> String {
    "user".into()
}

fn default_sync_on_startup() -> bool {
    true
}

impl BackendPref {
    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "авто",
            Self::Nftables => "nftables",
            Self::Iptables => "iptables",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Auto => Self::Nftables,
            Self::Nftables => Self::Iptables,
            Self::Iptables => Self::Auto,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Self::Auto => Self::Iptables,
            Self::Nftables => Self::Auto,
            Self::Iptables => Self::Nftables,
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            blocked: Vec::new(),
            backend: BackendPref::default(),
            apply_on_boot: false,
            sync_on_startup: default_sync_on_startup(),
            api_login: default_api_login(),
        }
    }
}

impl Settings {
    pub fn load() -> Result<Self> {
        let path = config_path()?;
        if !path.exists() {
            return Ok(Self::default());
        }

        let content = fs::read_to_string(&path)
            .with_context(|| format!("не удалось прочитать {}", path.display()))?;
        Ok(serde_json::from_str(&content)?)
    }

    pub fn save(&self) -> Result<()> {
        let path = config_path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let content = serde_json::to_string_pretty(self)?;
        fs::write(&path, content)?;
        Ok(())
    }

    pub fn resolve_backend(&self) -> Result<Backend> {
        match self.backend {
            BackendPref::Auto => crate::firewall::detect_backend(),
            BackendPref::Nftables => Ok(Backend::Nftables),
            BackendPref::Iptables => Ok(Backend::Iptables),
        }
    }
}

pub fn config_path() -> Result<PathBuf> {
    let dirs = directories::ProjectDirs::from("", "", "stalzone-server-blocker")
        .context("не удалось определить каталог конфигурации")?;
    Ok(dirs.config_dir().join("config.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_settings_are_empty() {
        let settings = Settings::default();
        assert!(settings.blocked.is_empty());
        assert!(settings.sync_on_startup);
        assert_eq!(settings.api_login, "user");
    }
}

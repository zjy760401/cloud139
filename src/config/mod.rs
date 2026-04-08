use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use thiserror::Error;

use crate::client::StorageType;

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("TOML error: {0}")]
    TomlDe(#[from] toml::de::Error),
    #[error("TOML serialize error: {0}")]
    TomlSer(#[from] toml::ser::Error),
    #[error("Config not found")]
    NotFound,
    #[error("Cannot determine config directory")]
    NoConfigDir,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub authorization: String,
    pub account: String,
    #[serde(default)]
    pub storage_type: String,
    pub cloud_id: Option<String>,
    #[serde(default)]
    pub custom_upload_part_size: i64,
    #[serde(default = "default_true")]
    pub report_real_size: bool,
    #[serde(default)]
    pub use_large_thumbnail: bool,
    #[serde(default)]
    pub personal_cloud_host: Option<String>,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub token_expire_time: Option<i64>,
    #[serde(default)]
    pub root_folder_id: Option<String>,
    #[serde(default)]
    pub user_domain_id: Option<String>,
}

fn default_true() -> bool {
    true
}

impl Default for Config {
    fn default() -> Self {
        Self {
            authorization: String::new(),
            account: String::new(),
            storage_type: String::new(),
            cloud_id: None,
            custom_upload_part_size: 0,
            report_real_size: default_true(),
            use_large_thumbnail: false,
            personal_cloud_host: None,
            refresh_token: None,
            token_expire_time: None,
            root_folder_id: None,
            user_domain_id: None,
        }
    }
}

impl Config {
    pub fn config_path() -> Result<PathBuf, ConfigError> {
        let home = std::env::var("HOME")
            .map(PathBuf::from)
            .map_err(|_| ConfigError::NoConfigDir)?;
        Ok(home.join(".config").join("cloud139").join("config.toml"))
    }

    pub fn load() -> Result<Self, ConfigError> {
        // 优先读取用户配置目录，回退到当前目录的旧路径
        let user_path = Self::config_path()?;
        let legacy_path = PathBuf::from("./cloud139.toml");

        let path = if user_path.exists() {
            user_path
        } else if legacy_path.exists() {
            legacy_path
        } else {
            return Err(ConfigError::NotFound);
        };

        let content = fs::read_to_string(&path)?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }

    pub fn save(&self) -> Result<(), ConfigError> {
        let path = Self::config_path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(self)?;
        fs::write(&path, content)?;
        Ok(())
    }

    pub fn storage_type(&self) -> StorageType {
        match self.storage_type.as_str() {
            "family" => StorageType::Family,
            "group" => StorageType::Group,
            _ => StorageType::PersonalNew,
        }
    }

    pub fn is_token_expired(&self) -> bool {
        if let Some(expire_time) = self.token_expire_time {
            let now = chrono::Utc::now().timestamp_millis();
            expire_time - now < 15 * 24 * 60 * 60 * 1000
        } else {
            true
        }
    }
}

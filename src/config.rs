use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub defaults: Defaults,
    #[serde(default)]
    pub profiles: ProfilesConfig,
    #[serde(default)]
    pub output: OutputConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Defaults {
    #[serde(default = "default_level")]
    pub level: String,
    #[serde(default = "default_sample_info")]
    pub sample_info: usize,
    #[serde(default)]
    pub sample_debug: usize,
}

fn default_level() -> String {
    "normal".to_string()
}

fn default_sample_info() -> usize {
    20
}

impl Default for Defaults {
    fn default() -> Self {
        Self {
            level: default_level(),
            sample_info: default_sample_info(),
            sample_debug: 0,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProfilesConfig {
    #[serde(default = "default_true")]
    pub auto_detect: bool,
    pub profiles_dir: Option<String>,
}

fn default_true() -> bool {
    true
}

impl Default for ProfilesConfig {
    fn default() -> Self {
        Self {
            auto_detect: true,
            profiles_dir: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct OutputConfig {
    #[serde(default)]
    pub show_stats: bool,
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self { show_stats: false }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            defaults: Defaults::default(),
            profiles: ProfilesConfig::default(),
            output: OutputConfig::default(),
        }
    }
}

pub fn load() -> Config {
    if let Some(path) = config_path() {
        if path.exists() {
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(config) = toml::from_str::<Config>(&content) {
                    return config;
                }
            }
        }
    }
    Config::default()
}

pub fn config_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".logslim").join("config.toml"))
}

pub fn profiles_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".logslim")
        .join("profiles")
}

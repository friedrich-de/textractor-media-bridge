use anyhow::{Context, Result};
use bridge_protocol::RangeScreenshotPick;
use serde::{Deserialize, Serialize};
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub server: ServerConfig,
    pub pipe: PipeConfig,
    pub screenshot: ScreenshotConfig,
    pub audio: AudioConfig,
    pub lines: LinesConfig,
    pub assets: AssetsConfig,
    pub mining: MiningConfig,
    pub anki: AnkiConfig,
    pub storage: StorageConfig,
}

impl AppConfig {
    pub fn default_path() -> PathBuf {
        PathBuf::from("config").join("bridge.toml")
    }

    pub fn load(path: Option<&Path>) -> Result<Self> {
        let Some(path) = path else {
            return Ok(Self::default());
        };
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read config {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("failed to parse config {}", path.display()))
    }

    pub fn load_from_default_locations(
        explicit: Option<PathBuf>,
    ) -> Result<(Self, Option<PathBuf>)> {
        if let Some(path) = explicit {
            return Ok((Self::load(Some(&path))?, Some(path)));
        }

        if let Some(path) = std::env::var_os("TEXTRACTOR_MEDIA_BRIDGE_CONFIG").map(PathBuf::from) {
            return Ok((Self::load(Some(&path))?, Some(path)));
        }

        let local = Self::default_path();
        if local.exists() {
            return Ok((Self::load(Some(&local))?, Some(local)));
        }

        Ok((Self::default(), None))
    }

    pub fn save_to_path(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("failed to create config directory {}", parent.display())
            })?;
        }
        let text = toml::to_string_pretty(self).context("failed to serialize config")?;
        std::fs::write(path, text)
            .with_context(|| format!("failed to write config {}", path.display()))
    }

    pub fn bind_addr(&self) -> SocketAddr {
        self.server
            .bind
            .parse()
            .unwrap_or_else(|_| SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 7788))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    pub bind: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind: "0.0.0.0:7788".to_owned(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PipeConfig {
    pub name: String,
}

impl Default for PipeConfig {
    fn default() -> Self {
        Self {
            name: "auto".to_owned(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ScreenshotConfig {
    pub backend: String,
}

impl Default for ScreenshotConfig {
    fn default() -> Self {
        Self {
            backend: "auto".to_owned(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioConfig {
    pub backend: String,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            backend: "auto".to_owned(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LinesConfig {
    pub join_progressive_text: bool,
}

impl Default for LinesConfig {
    fn default() -> Self {
        Self {
            join_progressive_text: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AssetsConfig {
    pub ttl_minutes: u64,
    pub max_storage_mb: u64,
}

impl Default for AssetsConfig {
    fn default() -> Self {
        Self {
            ttl_minutes: 120,
            max_storage_mb: 1024,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MiningConfig {
    pub screenshot_quality: u8,
    pub audio_bitrate_kbps: u32,
    pub ffmpeg_path: Option<PathBuf>,
}

impl Default for MiningConfig {
    fn default() -> Self {
        Self {
            screenshot_quality: 85,
            audio_bitrate_kbps: 96,
            ffmpeg_path: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AnkiConfig {
    pub range_sentence_separator: String,
    pub range_screenshot_pick: RangeScreenshotPick,
}

impl Default for AnkiConfig {
    fn default() -> Self {
        Self {
            range_sentence_separator: " ".to_owned(),
            range_screenshot_pick: RangeScreenshotPick::Last,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct StorageConfig {
    pub data_dir: Option<PathBuf>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_config_uses_slim_schema() {
        let text = toml::to_string_pretty(&AppConfig::default()).expect("config serializes");
        let value: toml::Value = toml::from_str(&text).expect("saved config parses");

        assert_eq!(
            value["server"]
                .as_table()
                .unwrap()
                .keys()
                .collect::<Vec<_>>(),
            vec!["bind"]
        );
        assert_eq!(
            value["audio"]
                .as_table()
                .unwrap()
                .keys()
                .collect::<Vec<_>>(),
            vec!["backend"]
        );
        assert_eq!(
            value["lines"]
                .as_table()
                .unwrap()
                .keys()
                .collect::<Vec<_>>(),
            vec!["join_progressive_text"]
        );
        assert!(text.contains("range_sentence_separator"));
        assert!(text.contains("audio_bitrate_kbps"));
        assert!(text.contains("join_progressive_text = true"));
    }

    #[test]
    fn default_config_binds_lan() {
        let config = AppConfig::default();

        assert_eq!(config.server.bind, "0.0.0.0:7788");
    }

    #[test]
    fn old_config_without_lines_defaults_to_progressive_joining() {
        let config: AppConfig = toml::from_str(
            r#"
            [server]
            bind = "0.0.0.0:7788"

            [audio]
            backend = "off"
            "#,
        )
        .expect("old config should parse");

        assert!(config.lines.join_progressive_text);
    }
}

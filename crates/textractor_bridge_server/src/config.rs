use anyhow::{Context, Result};
use bridge_protocol::RangeScreenshotPick;
use serde::{Deserialize, Serialize};
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub server: ServerConfig,
    pub pipe: PipeConfig,
    pub screenshot: ScreenshotConfig,
    pub audio: AudioConfig,
    pub assets: AssetsConfig,
    pub mining: MiningConfig,
    pub anki: AnkiConfig,
    pub storage: StorageConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            pipe: PipeConfig::default(),
            screenshot: ScreenshotConfig::default(),
            audio: AudioConfig::default(),
            assets: AssetsConfig::default(),
            mining: MiningConfig::default(),
            anki: AnkiConfig::default(),
            storage: StorageConfig::default(),
        }
    }
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
    pub lan_mode: bool,
    pub session_token_required: bool,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind: "0.0.0.0:7788".to_owned(),
            lan_mode: true,
            session_token_required: false,
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
    pub ready_preroll_ms: u64,
    pub trailing_silence_ms: u64,
    pub no_speech_timeout_ms: u64,
    pub trim_source_preroll_ms: u64,
    pub trim_trailing_silence_ms: u64,
    pub trim_no_speech_timeout_ms: u64,
    pub activity_threshold: u16,
    pub min_activity_ms: u64,
    pub trim_activity_threshold: u16,
    pub trim_min_activity_ms: u64,
    pub trim_padding_ms: u64,
    pub max_duration_ms: u64,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            backend: "auto".to_owned(),
            ready_preroll_ms: 1_000,
            trailing_silence_ms: 3_000,
            no_speech_timeout_ms: 5_000,
            trim_source_preroll_ms: 1_000,
            trim_trailing_silence_ms: 5_000,
            trim_no_speech_timeout_ms: 8_000,
            activity_threshold: 300,
            min_activity_ms: 30,
            trim_activity_threshold: 300,
            trim_min_activity_ms: 30,
            trim_padding_ms: 1_000,
            max_duration_ms: 120_000,
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
    fn loads_old_config_with_removed_fields() {
        let text = r#"
[server]
bind = "127.0.0.1:7788"
lan_mode = false

[screenshot]
backend = "auto"
format = "png"

[audio]
backend = "auto"
vad = "webrtc"
format = "wav"
ready_preroll_ms = 1000

[mining]
screenshot_format = "jpeg"
screenshot_quality = 85
audio_format = "mp3"
audio_bitrate_kbps = 96

[anki]
endpoint = "http://127.0.0.1:8765"
mode = "update_latest"
overwrite_sentence_field = false
fallback_create_note = false
range_sentence_separator = " "
range_screenshot_pick = "last"
deck_name = "Mining"
model_name = "Basic"
sentence_field = "Sentence"
notes_field = "Notes"
screenshot_field = "Image"
audio_field = "Audio"
source_field = "Source"
tags = ["textractor", "mined"]
"#;

        let config: AppConfig = toml::from_str(text).expect("old config should load");

        assert_eq!(config.audio.backend, "auto");
        assert_eq!(config.audio.ready_preroll_ms, 1_000);
        assert_eq!(config.mining.screenshot_quality, 85);
        assert_eq!(config.anki.range_screenshot_pick, RangeScreenshotPick::Last);
    }

    #[test]
    fn saved_config_uses_slim_schema() {
        let text = toml::to_string_pretty(&AppConfig::default()).expect("config serializes");

        for removed in [
            "vad",
            "screenshot_format",
            "audio_format",
            "overwrite_sentence_field",
            "fallback_create_note",
            "deck_name",
            "notes_field",
            "tags",
        ] {
            assert!(
                !text.contains(removed),
                "saved config should not contain removed field {removed}"
            );
        }
        assert!(text.contains("range_sentence_separator"));
        assert!(text.contains("audio_bitrate_kbps"));
    }

    #[test]
    fn default_config_binds_lan_without_session_token() {
        let config = AppConfig::default();

        assert_eq!(config.server.bind, "0.0.0.0:7788");
        assert!(config.server.lan_mode);
        assert!(!config.server.session_token_required);
    }
}

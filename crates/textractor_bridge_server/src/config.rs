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
            bind: "127.0.0.1:7788".to_owned(),
            lan_mode: false,
            session_token_required: true,
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
    pub format: String,
}

impl Default for ScreenshotConfig {
    fn default() -> Self {
        Self {
            backend: "auto".to_owned(),
            format: "png".to_owned(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioConfig {
    pub backend: String,
    pub vad: String,
    pub format: String,
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
            vad: "webrtc".to_owned(),
            format: "wav".to_owned(),
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
    pub screenshot_format: String,
    pub screenshot_quality: u8,
    pub audio_format: String,
    pub audio_bitrate_kbps: u32,
    pub ffmpeg_path: Option<PathBuf>,
}

impl Default for MiningConfig {
    fn default() -> Self {
        Self {
            screenshot_format: "jpeg".to_owned(),
            screenshot_quality: 85,
            audio_format: "mp3".to_owned(),
            audio_bitrate_kbps: 96,
            ffmpeg_path: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AnkiConfig {
    pub endpoint: String,
    pub mode: String,
    pub max_latest_card_age_minutes: u64,
    pub overwrite_sentence_field: bool,
    pub fallback_create_note: bool,
    pub range_sentence_separator: String,
    pub range_screenshot_pick: RangeScreenshotPick,
    pub deck_name: String,
    pub model_name: String,
    pub sentence_field: String,
    pub notes_field: String,
    pub screenshot_field: String,
    pub audio_field: String,
    pub source_field: String,
    pub tags: Vec<String>,
}

impl Default for AnkiConfig {
    fn default() -> Self {
        Self {
            endpoint: "http://127.0.0.1:8765".to_owned(),
            mode: "update_latest".to_owned(),
            max_latest_card_age_minutes: 5,
            overwrite_sentence_field: false,
            fallback_create_note: false,
            range_sentence_separator: " ".to_owned(),
            range_screenshot_pick: RangeScreenshotPick::Last,
            deck_name: "Mining".to_owned(),
            model_name: "Basic".to_owned(),
            sentence_field: "Sentence".to_owned(),
            notes_field: "Notes".to_owned(),
            screenshot_field: "Image".to_owned(),
            audio_field: "Audio".to_owned(),
            source_field: "Source".to_owned(),
            tags: vec!["textractor".to_owned(), "mined".to_owned()],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct StorageConfig {
    pub data_dir: Option<PathBuf>,
}

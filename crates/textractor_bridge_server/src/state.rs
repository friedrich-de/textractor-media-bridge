mod audio;
mod cleanup;
mod lines;
mod mining;
mod screenshots;

use anyhow::{Context, Result};
use bridge_protocol::{
    BrowserEvent, BrowserLineDeletedEvent, LineId, LinePatch, LineSeq, LineUpdatedEvent,
    LinesClearedEvent,
};
use parking_lot::RwLock;
use std::{path::PathBuf, sync::Arc};
use tokio::sync::broadcast;

use crate::{
    assets::AssetStore,
    config::{AppConfig, AudioConfig, LinesConfig},
    history::HistoryStore,
    media::{audio::AudioManager, screenshot::ScreenshotManager},
};

#[derive(Debug, Clone)]
pub struct SseMessage {
    pub id: LineSeq,
    pub event_name: &'static str,
    pub payload: BrowserEvent,
}

#[derive(Clone)]
pub struct AppState {
    inner: Arc<AppStateInner>,
}

struct AppStateInner {
    config: RwLock<AppConfig>,
    config_path: PathBuf,
    dirs: AppDirs,
    history: HistoryStore,
    assets: AssetStore,
    screenshots: ScreenshotManager,
    audio: AudioManager,
    events: broadcast::Sender<SseMessage>,
    websocket_text: broadcast::Sender<String>,
}

#[derive(Debug, Clone)]
pub struct AppDirs {
    pub root: PathBuf,
    pub history_path: PathBuf,
    pub assets_dir: PathBuf,
}

impl AppDirs {
    pub fn from_config(config: &AppConfig) -> Result<Self> {
        let root = config.storage.data_dir.clone().unwrap_or_else(|| {
            dirs::data_local_dir()
                .unwrap_or_else(|| PathBuf::from("data"))
                .join("TextractorMediaBridge")
        });
        let history_path = root.join("history.jsonl");
        let assets_dir = root.join("assets");
        std::fs::create_dir_all(&assets_dir)
            .with_context(|| format!("failed to create {}", assets_dir.display()))?;
        Ok(Self {
            root,
            history_path,
            assets_dir,
        })
    }
}

impl AppState {
    #[cfg(test)]
    pub fn load(config: AppConfig) -> Result<Self> {
        Self::load_with_config_path(config, None)
    }

    pub fn load_with_config_path(config: AppConfig, config_path: Option<PathBuf>) -> Result<Self> {
        let dirs = AppDirs::from_config(&config)?;
        let history = HistoryStore::load(dirs.history_path.clone())?;
        let assets = AssetStore::new(dirs.assets_dir.clone(), config.assets.clone())?;
        let screenshots = ScreenshotManager::new(config.screenshot.backend.clone());
        let audio = AudioManager::new(config.audio.clone());
        let (events, _) = broadcast::channel(512);
        let (websocket_text, _) = broadcast::channel(512);

        Ok(Self {
            inner: Arc::new(AppStateInner {
                config: RwLock::new(config),
                config_path: config_path.unwrap_or_else(AppConfig::default_path),
                dirs,
                history,
                assets,
                screenshots,
                audio,
                events,
                websocket_text,
            }),
        })
    }

    pub fn config(&self) -> AppConfig {
        self.inner.config.read().clone()
    }

    pub fn update_editable_config(
        &self,
        audio: AudioConfig,
        lines: LinesConfig,
    ) -> Result<AppConfig> {
        let mut updated = self.config();
        updated.audio = audio.clone();
        updated.lines = lines;
        updated.save_to_path(&self.inner.config_path)?;
        *self.inner.config.write() = updated.clone();
        self.inner.audio.update_config(audio);
        Ok(updated)
    }

    pub fn dirs(&self) -> &AppDirs {
        &self.inner.dirs
    }

    pub fn pipe_name(&self) -> String {
        let config = self.inner.config.read();
        if config.pipe.name == "auto" {
            bridge_protocol::default_pipe_name()
        } else {
            config.pipe.name.clone()
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<SseMessage> {
        self.inner.events.subscribe()
    }

    pub fn subscribe_websocket_text(&self) -> broadcast::Receiver<String> {
        self.inner.websocket_text.subscribe()
    }

    fn broadcast_websocket_text(&self, text: String) {
        let _ = self.inner.websocket_text.send(text);
    }

    fn broadcast_line_update(&self, line_seq: LineSeq, line_id: LineId, patch: LinePatch) {
        self.broadcast(
            line_seq,
            "line_updated",
            BrowserEvent::LineUpdated(LineUpdatedEvent {
                line_id,
                line_seq,
                patch,
            }),
        );
    }

    fn broadcast_lines_cleared(&self, id: LineSeq, cleared_lines: usize) {
        self.broadcast(
            id,
            "lines_cleared",
            BrowserEvent::LinesCleared(LinesClearedEvent { cleared_lines }),
        );
    }

    fn broadcast_line_deleted(&self, id: LineSeq, line_id: LineId) {
        self.broadcast(
            id,
            "line_deleted",
            BrowserEvent::LineDeleted(BrowserLineDeletedEvent { line_id }),
        );
    }

    fn broadcast(&self, id: LineSeq, event_name: &'static str, payload: BrowserEvent) {
        let _ = self.inner.events.send(SseMessage {
            id,
            event_name,
            payload,
        });
    }
}

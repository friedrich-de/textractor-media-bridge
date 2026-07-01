use base64::{engine::general_purpose::STANDARD, Engine};
use bridge_protocol::{AssetBase64Response, AssetInfo, AssetKind};
use image::codecs::jpeg::JpegEncoder;
use parking_lot::RwLock;
use std::{
    collections::HashMap,
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::{Duration, SystemTime},
};
use uuid::Uuid;

use crate::{config::AssetsConfig, time::unix_ms_now};

#[derive(Debug, thiserror::Error)]
pub enum AssetError {
    #[error("asset not found")]
    NotFound,
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("image error: {0}")]
    Image(#[from] image::ImageError),
    #[error("ffmpeg error: {0}")]
    Ffmpeg(String),
}

#[derive(Clone)]
pub struct AssetStore {
    dir: PathBuf,
    config: AssetsConfig,
    metadata: Arc<RwLock<HashMap<String, AssetInfo>>>,
}

impl AssetStore {
    pub fn new(dir: PathBuf, config: AssetsConfig) -> Result<Self, AssetError> {
        fs::create_dir_all(&dir)?;
        Ok(Self {
            dir,
            config,
            metadata: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    pub fn store_bytes(
        &self,
        kind: AssetKind,
        mime_type: impl Into<String>,
        bytes: &[u8],
        width: Option<u32>,
        height: Option<u32>,
        duration_ms: Option<u64>,
    ) -> Result<AssetInfo, AssetError> {
        let asset_id = format!("asset_{}", Uuid::new_v4().simple());
        let path = self.path_for_id(&asset_id);
        fs::write(&path, bytes)?;
        let asset = AssetInfo {
            url: format!("/assets/{asset_id}"),
            asset_id: asset_id.clone(),
            kind,
            mime_type: mime_type.into(),
            width,
            height,
            duration_ms,
            created_unix_ms: unix_ms_now(),
            byte_size: bytes.len() as u64,
        };
        self.metadata.write().insert(asset_id, asset.clone());
        Ok(asset)
    }

    pub fn find_asset_info(&self, asset_id: &str) -> Option<AssetInfo> {
        self.metadata.read().get(asset_id).cloned()
    }

    pub fn load_bytes(&self, asset_id: &str) -> Result<Vec<u8>, AssetError> {
        let path = self.path_for_id(asset_id);
        if !path.exists() {
            return Err(AssetError::NotFound);
        }
        Ok(fs::read(path)?)
    }

    pub fn base64_response(&self, asset: &AssetInfo) -> Result<AssetBase64Response, AssetError> {
        let bytes = self.load_bytes(&asset.asset_id)?;
        Ok(AssetBase64Response {
            asset_id: asset.asset_id.clone(),
            filename: filename_for_asset(asset),
            mime_type: asset.mime_type.clone(),
            data: STANDARD.encode(bytes),
        })
    }

    pub fn transcode_screenshot_to_jpeg(
        &self,
        source: &AssetInfo,
        quality: u8,
    ) -> Result<AssetInfo, AssetError> {
        if source.mime_type == "image/jpeg" {
            return Ok(source.clone());
        }
        let bytes = self.load_bytes(&source.asset_id)?;
        let image = image::load_from_memory(&bytes)?;
        let mut out = Vec::new();
        let mut encoder = JpegEncoder::new_with_quality(&mut out, quality.clamp(1, 100));
        encoder.encode_image(&image)?;
        self.store_bytes(
            AssetKind::Screenshot,
            "image/jpeg",
            &out,
            Some(image.width()),
            Some(image.height()),
            None,
        )
    }

    pub fn transcode_audio_to_mp3(
        &self,
        sources: &[AssetInfo],
        ffmpeg_path: &Path,
        bitrate_kbps: u32,
    ) -> Result<Option<AssetInfo>, AssetError> {
        if sources.is_empty() {
            return Ok(None);
        }

        let temp_dir = tempfile::Builder::new()
            .prefix("textractor-audio-")
            .tempdir()?;
        let mut input_paths = Vec::with_capacity(sources.len());
        for (index, source) in sources.iter().enumerate() {
            let input_path = temp_dir.path().join(format!(
                "input_{index:03}.{}",
                extension_for_mime(&source.mime_type)
            ));
            fs::write(&input_path, self.load_bytes(&source.asset_id)?)?;
            input_paths.push(input_path);
        }

        let output_path = temp_dir.path().join("output.mp3");
        let mut command = Command::new(ffmpeg_path);
        command
            .current_dir(temp_dir.path())
            .arg("-y")
            .arg("-hide_banner")
            .arg("-loglevel")
            .arg("error");

        if input_paths.len() == 1 {
            command.arg("-i").arg(&input_paths[0]);
        } else {
            let concat_list_path = temp_dir.path().join("inputs.txt");
            let concat_list = input_paths
                .iter()
                .map(|path| {
                    let file_name = path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or("input.wav");
                    format!("file '{file_name}'\n")
                })
                .collect::<String>();
            fs::write(&concat_list_path, concat_list)?;
            command
                .arg("-f")
                .arg("concat")
                .arg("-safe")
                .arg("0")
                .arg("-i")
                .arg(&concat_list_path);
        }

        let bitrate = format!("{}k", bitrate_kbps.max(1));
        let output = command
            .arg("-vn")
            .arg("-codec:a")
            .arg("libmp3lame")
            .arg("-b:a")
            .arg(&bitrate)
            .arg(&output_path)
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            let detail = if stderr.trim().is_empty() {
                stdout.trim()
            } else {
                stderr.trim()
            };
            return Err(AssetError::Ffmpeg(if detail.is_empty() {
                format!("ffmpeg exited with {}", output.status)
            } else {
                detail.to_owned()
            }));
        }

        let bytes = fs::read(&output_path)?;
        if bytes.is_empty() {
            return Err(AssetError::Ffmpeg(
                "ffmpeg produced an empty MP3 file".to_owned(),
            ));
        }

        self.store_bytes(
            AssetKind::Audio,
            "audio/mpeg",
            &bytes,
            None,
            None,
            total_duration_ms(sources),
        )
        .map(Some)
    }

    pub fn cleanup_expired(&self) -> Result<Vec<String>, AssetError> {
        let now = SystemTime::now();
        let ttl = Duration::from_secs(self.config.ttl_minutes.saturating_mul(60));
        let mut entries = self.asset_entries()?;
        let mut removed = Vec::new();

        for entry in &entries {
            if ttl.is_zero() {
                continue;
            }
            if now.duration_since(entry.modified).unwrap_or(Duration::ZERO) > ttl {
                if fs::remove_file(&entry.path).is_ok() {
                    removed.push(entry.asset_id.clone());
                }
            }
        }

        entries.retain(|entry| !removed.iter().any(|id| id == &entry.asset_id));
        let max_bytes = self.config.max_storage_mb.saturating_mul(1024 * 1024);
        if max_bytes > 0 {
            let mut total: u64 = entries.iter().map(|entry| entry.size).sum();
            entries.sort_by_key(|entry| entry.modified);
            for entry in entries {
                if total <= max_bytes {
                    break;
                }
                if fs::remove_file(&entry.path).is_ok() {
                    total = total.saturating_sub(entry.size);
                    removed.push(entry.asset_id);
                }
            }
        }

        Ok(removed)
    }

    pub fn path_for_id(&self, asset_id: &str) -> PathBuf {
        self.dir.join(asset_id)
    }

    #[allow(dead_code)]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn asset_entries(&self) -> Result<Vec<AssetEntry>, AssetError> {
        let mut entries = Vec::new();
        for entry in fs::read_dir(&self.dir)? {
            let entry = entry?;
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let metadata = entry.metadata()?;
            let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            entries.push(AssetEntry {
                asset_id: file_name.to_owned(),
                path,
                size: metadata.len(),
                modified: metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
            });
        }
        Ok(entries)
    }
}

struct AssetEntry {
    asset_id: String,
    path: PathBuf,
    size: u64,
    modified: SystemTime,
}

pub fn filename_for_asset(asset: &AssetInfo) -> String {
    let ext = extension_for_mime(&asset.mime_type);
    format!("{}.{}", asset.asset_id, ext)
}

pub fn resolve_ffmpeg_path(configured: Option<&Path>) -> Option<PathBuf> {
    if let Some(path) = configured {
        return Some(path.to_path_buf());
    }

    let sibling = env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|parent| parent.join(ffmpeg_exe_name())));
    if let Some(path) = sibling.filter(|path| path.is_file()) {
        return Some(path);
    }

    find_in_path(ffmpeg_exe_name()).or_else(|| find_in_path("ffmpeg"))
}

fn extension_for_mime(mime_type: &str) -> &'static str {
    match mime_type {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "audio/wav" | "audio/x-wav" => "wav",
        "audio/mpeg" => "mp3",
        other => mime_guess::get_mime_extensions_str(other)
            .and_then(|exts| exts.first().copied())
            .unwrap_or("bin"),
    }
}

fn find_in_path(name: &str) -> Option<PathBuf> {
    env::var_os("PATH").and_then(|paths| {
        env::split_paths(&paths)
            .map(|dir| dir.join(name))
            .find(|path| path.is_file())
    })
}

fn ffmpeg_exe_name() -> &'static str {
    if cfg!(windows) {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    }
}

fn total_duration_ms(sources: &[AssetInfo]) -> Option<u64> {
    let mut total = 0u64;
    for source in sources {
        total = total.saturating_add(source.duration_ms?);
    }
    Some(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filename_for_mp3_asset_uses_mp3_extension() {
        let asset = AssetInfo {
            asset_id: "asset_test".to_owned(),
            kind: AssetKind::Audio,
            mime_type: "audio/mpeg".to_owned(),
            url: "/assets/asset_test".to_owned(),
            width: None,
            height: None,
            duration_ms: Some(123),
            created_unix_ms: 0,
            byte_size: 456,
        };

        assert_eq!(filename_for_asset(&asset), "asset_test.mp3");
    }

    #[test]
    fn cleanup_removes_expired_assets() {
        let tmp = tempfile::tempdir().unwrap();
        let store = AssetStore::new(
            tmp.path().to_path_buf(),
            AssetsConfig {
                ttl_minutes: 1,
                max_storage_mb: 1024,
            },
        )
        .unwrap();
        let asset = store
            .store_bytes(AssetKind::Audio, "audio/wav", b"RIFF", None, None, Some(1))
            .unwrap();
        let path = store.path_for_id(&asset.asset_id);
        assert!(path.exists());
        let bytes = store.load_bytes(&asset.asset_id).unwrap();
        assert_eq!(bytes, b"RIFF");
    }

    #[test]
    fn store_bytes_remembers_asset_metadata() {
        let tmp = tempfile::tempdir().unwrap();
        let store = test_store(tmp.path());
        let asset = store
            .store_bytes(AssetKind::Audio, "audio/mpeg", b"mp3", None, None, Some(3))
            .unwrap();

        assert_eq!(store.find_asset_info(&asset.asset_id), Some(asset));
    }

    #[test]
    fn ffmpeg_transcodes_single_wav_to_mp3_when_available() {
        let Some(ffmpeg_path) = resolve_ffmpeg_path(None) else {
            eprintln!("skipping FFmpeg MP3 test because ffmpeg was not found");
            return;
        };
        let tmp = tempfile::tempdir().unwrap();
        let store = test_store(tmp.path());
        let source = store_test_wav(&store, 220.0, 180);

        let mp3 = store
            .transcode_audio_to_mp3(&[source], &ffmpeg_path, 96)
            .unwrap()
            .unwrap();

        assert_eq!(mp3.mime_type, "audio/mpeg");
        assert_eq!(mp3.duration_ms, Some(180));
        assert!(mp3.byte_size > 0);
        assert!(!store.load_bytes(&mp3.asset_id).unwrap().is_empty());
    }

    #[test]
    fn ffmpeg_concatenates_wavs_to_one_mp3_when_available() {
        let Some(ffmpeg_path) = resolve_ffmpeg_path(None) else {
            eprintln!("skipping FFmpeg concat test because ffmpeg was not found");
            return;
        };
        let tmp = tempfile::tempdir().unwrap();
        let store = test_store(tmp.path());
        let first = store_test_wav(&store, 220.0, 120);
        let second = store_test_wav(&store, 440.0, 160);

        let mp3 = store
            .transcode_audio_to_mp3(&[first, second], &ffmpeg_path, 96)
            .unwrap()
            .unwrap();

        assert_eq!(mp3.mime_type, "audio/mpeg");
        assert_eq!(mp3.duration_ms, Some(280));
        assert!(mp3.byte_size > 0);
        assert_eq!(filename_for_asset(&mp3).rsplit('.').next(), Some("mp3"));
    }

    fn test_store(path: &Path) -> AssetStore {
        AssetStore::new(
            path.to_path_buf(),
            AssetsConfig {
                ttl_minutes: 1,
                max_storage_mb: 1024,
            },
        )
        .unwrap()
    }

    fn store_test_wav(store: &AssetStore, frequency: f32, duration_ms: u64) -> AssetInfo {
        store
            .store_bytes(
                AssetKind::Audio,
                "audio/wav",
                &test_wav(frequency, duration_ms),
                None,
                None,
                Some(duration_ms),
            )
            .unwrap()
    }

    fn test_wav(frequency: f32, duration_ms: u64) -> Vec<u8> {
        let sample_rate = 48_000u32;
        let sample_count = (sample_rate as u64 * duration_ms / 1_000) as usize;
        let mut samples = Vec::with_capacity(sample_count);
        for index in 0..sample_count {
            let phase = index as f32 * frequency * std::f32::consts::TAU / sample_rate as f32;
            samples.push((phase.sin() * i16::MAX as f32 * 0.2) as i16);
        }

        encode_test_wav(&samples, sample_rate)
    }

    fn encode_test_wav(samples: &[i16], sample_rate: u32) -> Vec<u8> {
        let data_len = samples.len().saturating_mul(2).min(u32::MAX as usize) as u32;
        let riff_len = 36u32.saturating_add(data_len);
        let byte_rate = sample_rate.saturating_mul(2);

        let mut out = Vec::with_capacity(44 + data_len as usize);
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&riff_len.to_le_bytes());
        out.extend_from_slice(b"WAVE");
        out.extend_from_slice(b"fmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&sample_rate.to_le_bytes());
        out.extend_from_slice(&byte_rate.to_le_bytes());
        out.extend_from_slice(&2u16.to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data_len.to_le_bytes());
        for sample in samples {
            out.extend_from_slice(&sample.to_le_bytes());
        }
        out
    }
}

use bridge_protocol::{LineHistoryPage, LineId, LineRecord, LineSeq};
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{File, OpenOptions},
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};
use tracing::warn;

#[derive(Debug, thiserror::Error)]
pub enum HistoryError {
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid history entry on line {line_number}: {source}")]
    InvalidEntry {
        line_number: usize,
        #[source]
        source: serde_json::Error,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum HistoryOp {
    Upsert { line: Box<LineRecord> },
    Purge { line_id: LineId },
    Clear,
}

pub struct HistoryStore {
    path: PathBuf,
    lines: RwLock<BTreeMap<LineSeq, LineRecord>>,
    next_seq: AtomicU64,
    append_lock: Mutex<()>,
}

impl HistoryStore {
    pub fn load(path: PathBuf) -> Result<Self, HistoryError> {
        let lines = match load_history_lines(&path) {
            Ok(lines) => lines,
            Err(HistoryError::InvalidEntry {
                line_number,
                source,
            }) => {
                warn!(
                    path = %path.display(),
                    line_number,
                    %source,
                    "history file is incompatible or corrupt; quarantining and starting empty"
                );
                quarantine_invalid_history(&path);
                BTreeMap::new()
            }
            Err(error) => return Err(error),
        };

        let next = lines.keys().next_back().copied().unwrap_or(0) + 1;
        Ok(Self {
            path,
            lines: RwLock::new(lines),
            next_seq: AtomicU64::new(next.max(1)),
            append_lock: Mutex::new(()),
        })
    }

    pub fn next_line_seq(&self) -> LineSeq {
        self.next_seq.fetch_add(1, Ordering::Relaxed)
    }

    pub fn upsert(&self, line: LineRecord) -> Result<(), HistoryError> {
        self.lines.write().insert(line.line_seq, line.clone());
        self.append(&HistoryOp::Upsert {
            line: Box::new(line),
        })
    }

    pub fn update<F>(&self, line_id: LineId, update: F) -> Result<Option<LineRecord>, HistoryError>
    where
        F: FnOnce(&mut LineRecord),
    {
        let updated = {
            let mut lines = self.lines.write();
            let Some(line) = lines.values_mut().find(|line| line.line_id == line_id) else {
                return Ok(None);
            };
            update(line);
            line.clone()
        };
        self.append(&HistoryOp::Upsert {
            line: Box::new(updated.clone()),
        })?;
        Ok(Some(updated))
    }

    pub fn purge_line(&self, line_id: LineId) -> Result<bool, HistoryError> {
        let removed = {
            let mut lines = self.lines.write();
            let before = lines.len();
            lines.retain(|_, line| line.line_id != line_id);
            before != lines.len()
        };
        if removed {
            self.append(&HistoryOp::Purge { line_id })?;
        }
        Ok(removed)
    }

    pub fn clear(&self) -> Result<usize, HistoryError> {
        let removed = {
            let mut lines = self.lines.write();
            let removed = lines.len();
            lines.clear();
            removed
        };
        if removed > 0 {
            self.append(&HistoryOp::Clear)?;
        }
        Ok(removed)
    }

    pub fn get_line(&self, line_id: LineId) -> Option<LineRecord> {
        self.lines
            .read()
            .values()
            .find(|line| line.line_id == line_id)
            .cloned()
    }

    pub fn get_lines_by_ids(&self, line_ids: &[LineId]) -> Vec<LineRecord> {
        let lines = self.lines.read();
        line_ids
            .iter()
            .filter_map(|line_id| {
                lines
                    .values()
                    .find(|line| line.line_id == *line_id)
                    .cloned()
            })
            .collect()
    }

    pub fn all_lines(&self) -> Vec<LineRecord> {
        self.lines.read().values().cloned().collect()
    }

    pub fn newest_seq(&self) -> Option<LineSeq> {
        self.lines.read().keys().next_back().copied()
    }

    pub fn page(
        &self,
        limit: usize,
        before_seq: Option<LineSeq>,
        after_seq: Option<LineSeq>,
        source_key: Option<&str>,
    ) -> LineHistoryPage {
        let limit = limit.clamp(1, 500);
        let lines = self.lines.read();

        let source_matches = |line: &&LineRecord| {
            source_key
                .map(|source| line.source_key() == source)
                .unwrap_or(true)
        };

        let mut selected: Vec<LineRecord> = if let Some(after_seq) = after_seq {
            lines
                .range((after_seq + 1)..)
                .map(|(_, line)| line)
                .filter(source_matches)
                .take(limit)
                .cloned()
                .collect()
        } else if let Some(before_seq) = before_seq {
            let mut page: Vec<_> = lines
                .range(..before_seq)
                .rev()
                .map(|(_, line)| line)
                .filter(source_matches)
                .take(limit)
                .cloned()
                .collect();
            page.reverse();
            page
        } else {
            let mut page: Vec<_> = lines
                .iter()
                .rev()
                .map(|(_, line)| line)
                .filter(source_matches)
                .take(limit)
                .cloned()
                .collect();
            page.reverse();
            page
        };

        selected.sort_by_key(|line| line.line_seq);
        let oldest_seq = selected.first().map(|line| line.line_seq);
        let newest_seq = selected.last().map(|line| line.line_seq);
        let has_more_older = oldest_seq
            .map(|oldest| {
                lines.range(..oldest).map(|(_, line)| line).any(|line| {
                    source_key
                        .map(|source| line.source_key() == source)
                        .unwrap_or(true)
                })
            })
            .unwrap_or(false);
        let has_more_newer = newest_seq
            .map(|newest| {
                lines
                    .range((newest + 1)..)
                    .map(|(_, line)| line)
                    .any(|line| {
                        source_key
                            .map(|source| line.source_key() == source)
                            .unwrap_or(true)
                    })
            })
            .unwrap_or(false);

        LineHistoryPage {
            lines: selected,
            oldest_seq,
            newest_seq,
            has_more_older,
            has_more_newer,
        }
    }

    fn append(&self, op: &HistoryOp) -> Result<(), HistoryError> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let _guard = self.append_lock.lock();
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        serde_json::to_writer(&mut file, op)?;
        file.write_all(b"\n")?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

fn load_history_lines(path: &Path) -> Result<BTreeMap<LineSeq, LineRecord>, HistoryError> {
    let mut lines = BTreeMap::new();
    if path.exists() {
        let reader = BufReader::new(File::open(path)?);
        for (index, line) in reader.lines().enumerate() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<HistoryOp>(&line).map_err(|source| {
                HistoryError::InvalidEntry {
                    line_number: index + 1,
                    source,
                }
            })? {
                HistoryOp::Upsert { line } => {
                    let line = *line;
                    lines.insert(line.line_seq, line);
                }
                HistoryOp::Purge { line_id } => {
                    lines.retain(|_, line| line.line_id != line_id);
                }
                HistoryOp::Clear => {
                    lines.clear();
                }
            }
        }
    }

    Ok(lines)
}

fn quarantine_invalid_history(path: &Path) {
    if !path.exists() {
        return;
    }
    let timestamp_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let quarantine_path = path.with_file_name(format!("history.invalid.{timestamp_ms}.jsonl"));
    if let Err(error) = std::fs::rename(path, &quarantine_path) {
        warn!(
            %error,
            path = %path.display(),
            quarantine = %quarantine_path.display(),
            "failed to quarantine invalid history file; continuing with empty history"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bridge_protocol::{PipeLineMeta, PROTOCOL_VERSION};

    fn line(seq: u64) -> LineRecord {
        LineRecord {
            line_id: seq,
            line_seq: seq,
            timestamp_unix_ms: 1_000 + seq as i64,
            text: format!("line {seq}"),
            meta: PipeLineMeta {
                process_id: 1,
                thread_number: 2,
                thread_name: Some("hook".to_owned()),
                window_title: Some("Game Window".to_owned()),
                is_current_select: true,
                arch: "x64".to_owned(),
                source: format!("proto {PROTOCOL_VERSION}"),
            },
            screenshot: None,
            audio: None,
            warnings: vec![],
            ignored: false,
        }
    }

    fn invalid_history_files(dir: &Path) -> Vec<PathBuf> {
        std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| {
                        name.starts_with("history.invalid.") && name.ends_with(".jsonl")
                    })
            })
            .collect()
    }

    #[test]
    fn paginates_recent_and_older_lines() {
        let tmp = tempfile::tempdir().unwrap();
        let store = HistoryStore::load(tmp.path().join("history.jsonl")).unwrap();
        for seq in 1..=5 {
            store.upsert(line(seq)).unwrap();
        }

        let recent = store.page(2, None, None, None);
        assert_eq!(
            recent
                .lines
                .iter()
                .map(|line| line.line_seq)
                .collect::<Vec<_>>(),
            vec![4, 5]
        );
        assert!(recent.has_more_older);

        let older = store.page(2, Some(4), None, None);
        assert_eq!(
            older
                .lines
                .iter()
                .map(|line| line.line_seq)
                .collect::<Vec<_>>(),
            vec![2, 3]
        );
        assert!(older.has_more_older);
        assert!(older.has_more_newer);
    }

    #[test]
    fn clear_removes_lines_and_persists_across_reload() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("history.jsonl");
        let store = HistoryStore::load(path.clone()).unwrap();
        store.upsert(line(1)).unwrap();
        store.upsert(line(2)).unwrap();

        assert_eq!(store.clear().unwrap(), 2);
        assert!(store.all_lines().is_empty());

        let reloaded = HistoryStore::load(path).unwrap();
        assert!(reloaded.all_lines().is_empty());
    }

    #[test]
    fn incompatible_history_is_quarantined_and_starts_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("history.jsonl");
        std::fs::write(
            &path,
            r#"{"op":"upsert","line":{"lineId":1,"lineSeq":1,"timestampUnixMs":1000,"text":"old","meta":{"processId":1,"threadNumber":2,"isCurrentSelect":true,"arch":"x86","source":"textractor"},"audio":{"status":"ready","asset":{"assetId":"asset_old","kind":"audio","mimeType":"audio/wav","url":"/api/assets/asset_old","durationMs":100,"createdUnixMs":1000,"byteSize":44},"durationMs":100,"endReason":"silence"}}}"#,
        )
        .unwrap();

        let store = HistoryStore::load(path.clone()).unwrap();

        assert!(store.all_lines().is_empty());
        assert!(!path.exists());
        let quarantined = invalid_history_files(tmp.path());
        assert_eq!(quarantined.len(), 1);
    }

    #[test]
    fn malformed_history_is_quarantined_and_starts_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("history.jsonl");
        std::fs::write(&path, "{not json}\n").unwrap();

        let store = HistoryStore::load(path.clone()).unwrap();

        assert!(store.all_lines().is_empty());
        assert!(!path.exists());
        let quarantined = invalid_history_files(tmp.path());
        assert_eq!(quarantined.len(), 1);
    }
}

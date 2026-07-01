use anyhow::{anyhow, Context, Result};
use bridge_protocol::{AssetKind, LineId, LinePatch};
use tracing::{debug, warn};

use crate::media::{
    screenshot::{CapturedScreenshot, ScreenshotManager},
    window::{resolve_process_window, NativeHwnd},
};

use super::AppState;

impl AppState {
    pub(super) fn spawn_screenshot_capture(&self, line_id: LineId, process_id: u32) {
        let state = self.clone();
        tokio::spawn(async move {
            if let Err(error) = state.capture_screenshot_for_line(line_id, process_id).await {
                warn!(%error, line_id, process_id, "screenshot capture failed");
                let _ = state.add_warning(line_id, format!("screenshot capture failed: {error}"));
            }
        });
    }

    async fn capture_screenshot_for_line(&self, line_id: LineId, process_id: u32) -> Result<()> {
        let hwnd = resolve_process_window(process_id)
            .ok_or_else(|| anyhow!("no visible window found for process {process_id}"))?;
        let manager = self.inner.screenshots.clone();
        let captured = tokio::task::spawn_blocking(move || capture_with_manager(manager, hwnd))
            .await
            .context("screenshot worker panicked")??;
        debug!(
            line_id,
            process_id,
            backend = captured.backend,
            width = captured.width,
            height = captured.height,
            "screenshot captured"
        );

        let asset = self.inner.assets.store_bytes(
            AssetKind::Screenshot,
            "image/png",
            &captured.bytes,
            Some(captured.width),
            Some(captured.height),
            None,
        )?;

        let updated = self.inner.history.update(line_id, |line| {
            line.screenshot = Some(asset.clone());
        })?;
        if let Some(line) = updated {
            self.broadcast_line_update(
                line.line_seq,
                line_id,
                LinePatch {
                    screenshot: Some(line.screenshot.clone()),
                    warnings: Some(line.warnings.clone()),
                    ..LinePatch::default()
                },
            );
        }
        Ok(())
    }

    fn add_warning(&self, line_id: LineId, warning: String) -> Result<()> {
        let updated = self.inner.history.update(line_id, |line| {
            if !line.warnings.iter().any(|item| item == &warning) {
                line.warnings.push(warning.clone());
            }
        })?;
        if let Some(line) = updated {
            self.broadcast_line_update(
                line.line_seq,
                line_id,
                LinePatch {
                    warnings: Some(line.warnings.clone()),
                    ..LinePatch::default()
                },
            );
        }
        Ok(())
    }
}

fn capture_with_manager(
    manager: ScreenshotManager,
    hwnd: NativeHwnd,
) -> Result<CapturedScreenshot> {
    Ok(manager.capture_window(hwnd)?)
}

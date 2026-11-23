//! Screen region geometry, DPI conversion, overlap detection, and lightweight frame-diff gating.

use luma_core::{CaptureRegion, MonitorInfo};
use std::collections::HashMap;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum RegionError {
    #[error("Region dimensions must be at least 16x16 pixels (got {0}x{1})")]
    TooSmall(u32, u32),
    #[error("Region '{0}' does not intersect monitor '{1}'")]
    OutsideMonitor(String, String),
    #[error("Region '{0}' not found")]
    NotFound(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RectPx {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl RectPx {
    pub fn right(self) -> i64 {
        self.x as i64 + self.width as i64
    }

    pub fn bottom(self) -> i64 {
        self.y as i64 + self.height as i64
    }

    pub fn intersects(self, other: Self) -> bool {
        let ix0 = (self.x as i64).max(other.x as i64);
        let iy0 = (self.y as i64).max(other.y as i64);
        let ix1 = self.right().min(other.right());
        let iy1 = self.bottom().min(other.bottom());
        ix1 > ix0 && iy1 > iy0
    }

    pub fn clamp_to(self, bounds: Self) -> Option<Self> {
        let x0 = (self.x as i64).max(bounds.x as i64);
        let y0 = (self.y as i64).max(bounds.y as i64);
        let x1 = self.right().min(bounds.right());
        let y1 = self.bottom().min(bounds.bottom());
        if x1 <= x0 || y1 <= y0 {
            return None;
        }
        Some(Self {
            x: x0 as i32,
            y: y0 as i32,
            width: (x1 - x0) as u32,
            height: (y1 - y0) as u32,
        })
    }
}

/// Convert logical DIP coordinates into physical screen pixels for a given DPI scale (e.g. 1.25, 1.5).
pub fn logical_to_physical_rect(x: f64, y: f64, width: f64, height: f64, dpi_scale: f64) -> RectPx {
    let scale = if dpi_scale > 0.1 { dpi_scale } else { 1.0 };
    RectPx {
        x: (x * scale).round() as i32,
        y: (y * scale).round() as i32,
        width: (width * scale).round().max(1.0) as u32,
        height: (height * scale).round().max(1.0) as u32,
    }
}

/// Validate and clamp a user-selected `CaptureRegion` against known monitors.
pub fn validate_and_clamp_region(
    mut region: CaptureRegion,
    monitors: &[MonitorInfo],
) -> Result<CaptureRegion, RegionError> {
    if region.width < 16 || region.height < 16 {
        return Err(RegionError::TooSmall(region.width, region.height));
    }

    if monitors.is_empty() {
        return Ok(region);
    }

    let raw_rect = RectPx {
        x: region.x,
        y: region.y,
        width: region.width,
        height: region.height,
    };

    // Match explicit monitor_id first, or pick the monitor with largest intersection area.
    let chosen_monitor = monitors
        .iter()
        .find(|m| m.id == region.monitor_id)
        .filter(|m| {
            raw_rect.intersects(RectPx {
                x: m.x,
                y: m.y,
                width: m.width,
                height: m.height,
            })
        })
        .or_else(|| {
            monitors.iter().max_by_key(|m| {
                let mb = RectPx {
                    x: m.x,
                    y: m.y,
                    width: m.width,
                    height: m.height,
                };
                raw_rect
                    .clamp_to(mb)
                    .map(|c| c.width as u64 * c.height as u64)
                    .unwrap_or(0)
            })
        })
        .ok_or_else(|| RegionError::OutsideMonitor(region.id.clone(), region.monitor_id.clone()))?;

    let mon_bounds = RectPx {
        x: chosen_monitor.x,
        y: chosen_monitor.y,
        width: chosen_monitor.width,
        height: chosen_monitor.height,
    };
    let clamped = raw_rect
        .clamp_to(mon_bounds)
        .ok_or_else(|| RegionError::OutsideMonitor(region.id.clone(), chosen_monitor.id.clone()))?;

    if clamped.width < 16 || clamped.height < 16 {
        return Err(RegionError::TooSmall(clamped.width, clamped.height));
    }

    region.monitor_id = chosen_monitor.id.clone();
    region.dpi_scale = chosen_monitor.dpi_scale;
    region.x = clamped.x;
    region.y = clamped.y;
    region.width = clamped.width;
    region.height = clamped.height;
    Ok(region)
}

/// Lightweight per-region BGRA8 frame-difference detector that prevents redundant GPU OCR
/// on static subtitle frames while remaining sensitive to small dialogue character changes.
pub struct FrameDiffDetector {
    /// Minimum mean absolute difference (0..255) OR changed-pixel ratio to trigger OCR.
    mean_diff_threshold: f32,
    changed_pixel_ratio_threshold: f32,
    pixel_delta_cutoff: u8,
    previous_downsampled: HashMap<String, (u32, u32, Vec<u8>)>,
}

impl Default for FrameDiffDetector {
    fn default() -> Self {
        Self {
            mean_diff_threshold: 1.75,
            changed_pixel_ratio_threshold: 0.004, // 0.4% of sampled subtitle pixels changed
            pixel_delta_cutoff: 18,
            previous_downsampled: HashMap::new(),
        }
    }
}

impl FrameDiffDetector {
    pub fn new(
        mean_diff_threshold: f32,
        changed_pixel_ratio_threshold: f32,
        pixel_delta_cutoff: u8,
    ) -> Self {
        Self {
            mean_diff_threshold,
            changed_pixel_ratio_threshold,
            pixel_delta_cutoff,
            previous_downsampled: HashMap::new(),
        }
    }

    pub fn remove_region(&mut self, region_id: &str) {
        self.previous_downsampled.remove(region_id);
    }

    pub fn reset(&mut self) {
        self.previous_downsampled.clear();
    }

    /// Inspect a tightly packed BGRA8 crop (`width * height * 4` bytes) and return `true`
    /// if this is the first frame, dimensions changed, or pixel change exceeds threshold.
    pub fn has_changed(&mut self, region_id: &str, width: u32, height: u32, bgra: &[u8]) -> bool {
        let expected_len = (width as usize)
            .saturating_mul(height as usize)
            .saturating_mul(4);
        if width == 0 || height == 0 || bgra.len() < expected_len {
            return false;
        }

        // Subsample every 2x2 pixel luma value to keep CPU usage near zero (<0.05 ms for 900x120).
        let step_x = 2usize;
        let step_y = 2usize;
        let cols = (width as usize).div_ceil(step_x);
        let rows = (height as usize).div_ceil(step_y);
        let mut luma_grid = Vec::with_capacity(cols * rows);

        for y in (0..height as usize).step_by(step_y) {
            let row_offset = y * (width as usize) * 4;
            for x in (0..width as usize).step_by(step_x) {
                let idx = row_offset + x * 4;
                let b = bgra[idx] as u32;
                let g = bgra[idx + 1] as u32;
                let r = bgra[idx + 2] as u32;
                // Fast integer BT.601 luma approximation
                let y_val = ((r * 77 + g * 150 + b * 29) >> 8) as u8;
                luma_grid.push(y_val);
            }
        }

        let Some((prev_w, prev_h, prev_luma)) = self.previous_downsampled.get_mut(region_id) else {
            self.previous_downsampled
                .insert(region_id.to_string(), (width, height, luma_grid));
            return true;
        };

        if *prev_w != width || *prev_h != height || prev_luma.len() != luma_grid.len() {
            *prev_w = width;
            *prev_h = height;
            *prev_luma = luma_grid;
            return true;
        }

        let mut total_diff: u64 = 0;
        let mut changed_samples: usize = 0;
        for (&old_v, &new_v) in prev_luma.iter().zip(luma_grid.iter()) {
            let diff = old_v.abs_diff(new_v);
            total_diff += diff as u64;
            if diff >= self.pixel_delta_cutoff {
                changed_samples += 1;
            }
        }

        let sample_count = luma_grid.len().max(1) as f32;
        let mean_diff = total_diff as f32 / sample_count;
        let changed_ratio = changed_samples as f32 / sample_count;

        let changed = mean_diff >= self.mean_diff_threshold
            || changed_ratio >= self.changed_pixel_ratio_threshold;
        if changed {
            *prev_luma = luma_grid;
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamps_region_to_monitor_and_handles_dpi() {
        let phys = logical_to_physical_rect(100.0, 200.0, 400.0, 80.0, 1.5);
        assert_eq!(
            phys,
            RectPx {
                x: 150,
                y: 300,
                width: 600,
                height: 120
            }
        );

        let monitors = vec![MonitorInfo {
            id: "mon-1".into(),
            name: "Primary".into(),
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
            dpi_scale: 1.25,
            is_primary: true,
        }];
        let reg = CaptureRegion {
            id: "r1".into(),
            name: "Region 1".into(),
            monitor_id: "mon-1".into(),
            x: 1700,
            y: 1000,
            width: 400,
            height: 150,
            dpi_scale: 1.0,
            source_language: "ja".into(),
            enabled: true,
        };
        let validated = validate_and_clamp_region(reg, &monitors).unwrap();
        assert_eq!(validated.width, 220);
        assert_eq!(validated.height, 80);
        assert_eq!(validated.dpi_scale, 1.25);
    }

    #[test]
    fn frame_diff_skips_identical_frames_and_detects_subtitle_change() {
        let mut detector = FrameDiffDetector::default();
        let w = 200u32;
        let h = 60u32;
        let mut frame = vec![20u8; (w * h * 4) as usize];

        assert!(detector.has_changed("r1", w, h, &frame));
        assert!(!detector.has_changed("r1", w, h, &frame));

        // Draw bright subtitle glyph block onto 2% of pixels
        for y in 20..30 {
            for x in 40..90 {
                let idx = ((y * w + x) * 4) as usize;
                frame[idx] = 240;
                frame[idx + 1] = 240;
                frame[idx + 2] = 240;
            }
        }
        assert!(detector.has_changed("r1", w, h, &frame));
        assert!(!detector.has_changed("r1", w, h, &frame));
    }
}

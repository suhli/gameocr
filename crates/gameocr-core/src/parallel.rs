//! Pure planning and geometry shared by the desktop executor and benchmark.
use crate::types::{Device, OcrBlock, Point};
use anyhow::{Result, ensure};
use image::RgbImage;
use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub struct FrameInput {
    pub id: u64,
    pub timestamp: u64,
    pub generation: u64,
    pub image: RgbImage,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ParallelConfig {
    pub enable_parallel_inference: bool,
    pub enable_cpu_workers: bool,
    pub gpu_worker_count: usize,
    pub cpu_worker_count: usize,
}
impl Default for ParallelConfig {
    fn default() -> Self {
        Self {
            enable_parallel_inference: false,
            enable_cpu_workers: false,
            gpu_worker_count: 1,
            cpu_worker_count: 1,
        }
    }
}
impl ParallelConfig {
    pub fn validate(&self, device: Device, logical_cores: usize) -> Result<()> {
        ensure!(
            self.gpu_worker_count <= 2,
            "GPU Worker 最多 2 个；多个实例共享同一显卡"
        );
        ensure!(
            self.cpu_worker_count <= logical_cores.clamp(1, 4),
            "CPU Worker 不能超过逻辑核心数或 4 个"
        );
        if self.enable_parallel_inference {
            let (gpu, cpu) = self.requested(device);
            ensure!(gpu + cpu > 0, "当前设备模式下必须至少启用一个 Worker");
        }
        Ok(())
    }
    pub fn requested(&self, device: Device) -> (usize, usize) {
        match device {
            Device::Cpu => (0, self.cpu_worker_count),
            Device::Cuda => (self.gpu_worker_count, 0),
            Device::Auto => (
                self.gpu_worker_count,
                if self.enable_cpu_workers {
                    self.cpu_worker_count
                } else {
                    0
                },
            ),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ExecutionMetrics {
    pub batch_id: u64,
    pub end_to_end_ms: f64,
    pub transfer_ms: f64,
    pub split_ms: f64,
    pub queue_wait_ms: f64,
    pub worker_wall_ms: f64,
    pub merge_ms: f64,
    pub cpu_tasks: u32,
    pub gpu_tasks: u32,
    pub failed_tasks: u32,
    pub retried_tasks: u32,
    pub tile_count: usize,
    pub whole_frame_fallback: bool,
    // Filled only by the benchmark when an identical-input baseline exists.
    pub speedup: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskIdentity {
    pub batch_id: u64,
    pub task_id: u32,
    pub epoch: u64,
    pub generation: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tile {
    pub id: u32,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub detection_resize: u32,
}
impl Tile {
    pub fn whole(width: u32, height: u32, resize: u32) -> Self {
        Self {
            id: 0,
            x: 0,
            y: 0,
            width,
            height,
            detection_resize: resize,
        }
    }
    pub fn area(&self) -> f64 {
        self.width as f64 * self.height as f64
    }
}

pub struct FrameSplitter;
impl FrameSplitter {
    pub fn plan(image: &RgbImage, workers: usize, resize: u32) -> Vec<Tile> {
        let (w, h) = image.dimensions();
        let whole = || vec![Tile::whole(w, h, resize)];
        if workers < 2 || w < 1280 || h < 720 {
            return whole();
        }
        // Sample contrast rather than running another detector. Blank/sparse images
        // stay whole; this is an optimization hint, never an OCR result cache.
        let mut edges = [0usize; 4];
        let mut samples = 0;
        for y in (4..h).step_by(8) {
            for x in (4..w).step_by(8) {
                let a = image.get_pixel(x, y);
                let b = image.get_pixel(x - 4, y);
                let c = image.get_pixel(x, y - 4);
                if (0..3).any(|i| a[i].abs_diff(b[i]) > 40 || a[i].abs_diff(c[i]) > 40) {
                    edges[(y >= h / 2) as usize * 2 + (x >= w / 2) as usize] += 1;
                }
                samples += 1;
            }
        }
        if edges.iter().sum::<usize>() * 500 < samples
            || edges.iter().filter(|&&n| n >= 8).count() < 2
        {
            return whole();
        }
        let sx = quiet_seam(image, true);
        let sy = quiet_seam(image, false);
        const OVERLAP: u32 = 64;
        let xs = [(0, (sx + OVERLAP).min(w)), (sx.saturating_sub(OVERLAP), w)];
        let ys = [(0, (sy + OVERLAP).min(h)), (sy.saturating_sub(OVERLAP), h)];
        let mut tiles = Vec::new();
        for (y0, y1) in ys {
            for (x0, x1) in xs {
                let longest = (x1 - x0).max(y1 - y0);
                // Keep approximately the same input-pixel scale as whole-frame OCR.
                // Do not run every tile at the original full-frame detector budget.
                let edge = ((resize as f64 * longest as f64 / w.max(h) as f64 / 32.).ceil() as u32
                    * 32)
                    .clamp(32, resize);
                tiles.push(Tile {
                    id: tiles.len() as u32,
                    x: x0,
                    y: y0,
                    width: x1 - x0,
                    height: y1 - y0,
                    detection_resize: edge,
                });
            }
        }
        tiles
    }
}

fn quiet_seam(image: &RgbImage, vertical: bool) -> u32 {
    let (length, cross) = if vertical {
        image.dimensions()
    } else {
        (image.height(), image.width())
    };
    let mut best = (u64::MAX, length / 2);
    for coordinate in ((length * 45 / 100)..=(length * 55 / 100)).step_by(4) {
        let mut energy = 0u64;
        for p in (2..cross).step_by(4) {
            let (x, y) = if vertical {
                (coordinate, p)
            } else {
                (p, coordinate)
            };
            let a = image.get_pixel(x, y);
            let b = image.get_pixel(x - 2, y);
            let c = image.get_pixel(x, y - 2);
            energy += (0..3)
                .map(|i| a[i].abs_diff(b[i]) as u64 + a[i].abs_diff(c[i]) as u64)
                .sum::<u64>();
        }
        // Prefer central cuts among similarly quiet candidates.
        let score = energy * 100 + coordinate.abs_diff(length / 2) as u64;
        if score < best.0 {
            best = (score, coordinate);
        }
    }
    best.1
}

pub fn map_block(mut block: OcrBlock, tile: &Tile, source: (u32, u32)) -> Result<OcrBlock> {
    if let Some(b) = &mut block.bbox {
        ensure!(
            b.iter().all(|v| v.is_finite()) && b[0] <= b[2] && b[1] <= b[3],
            "Invalid tile bbox"
        );
        b[0] = (b[0] + tile.x as f64).clamp(0., source.0 as f64);
        b[2] = (b[2] + tile.x as f64).clamp(0., source.0 as f64);
        b[1] = (b[1] + tile.y as f64).clamp(0., source.1 as f64);
        b[3] = (b[3] + tile.y as f64).clamp(0., source.1 as f64);
    }
    if let Some(points) = &mut block.polygon {
        for Point { x, y } in points {
            ensure!(x.is_finite() && y.is_finite(), "Invalid tile polygon");
            *x = (*x + tile.x as f64).clamp(0., source.0 as f64);
            *y = (*y + tile.y as f64).clamp(0., source.1 as f64);
        }
    }
    Ok(block)
}

/// Border-touching text is uncertain, even if its CTC confidence is high.
/// The executor retries the entire frame before publishing instead of exposing
/// fragments or claiming a speedup on an incomplete recognition.
pub fn touches_internal_edge(block: &OcrBlock, tile: &Tile, source: (u32, u32)) -> bool {
    let Some(b) = block.bbox else {
        return true;
    };
    let guard = ((b[3] - b[1]) * 0.75).clamp(12., 48.);
    (tile.x > 0 && b[0] < guard)
        || (tile.y > 0 && b[1] < guard)
        || (tile.x + tile.width < source.0 && b[2] > tile.width as f64 - guard)
        || (tile.y + tile.height < source.1 && b[3] > tile.height as f64 - guard)
}

pub struct ResultMerger;
impl ResultMerger {
    pub fn merge(mut blocks: Vec<OcrBlock>) -> Vec<OcrBlock> {
        // Prefer longer complete strings, then their original confidence.
        blocks.sort_by(|a, b| {
            b.text
                .chars()
                .count()
                .cmp(&a.text.chars().count())
                .then_with(|| {
                    b.confidence
                        .unwrap_or(0.)
                        .total_cmp(&a.confidence.unwrap_or(0.))
                })
        });
        let mut kept: Vec<OcrBlock> = Vec::new();
        for block in blocks {
            if !kept.iter().any(|other| duplicate(other, &block)) {
                kept.push(block);
            }
        }
        kept.sort_by(|a, b| {
            let a = a.bbox.unwrap_or([0.; 4]);
            let b = b.bbox.unwrap_or([0.; 4]);
            a[1].total_cmp(&b[1]).then_with(|| a[0].total_cmp(&b[0]))
        });
        // Group nearby text baselines into rows, then read left to right.
        let mut start = 0;
        while start < kept.len() {
            let first = kept[start].bbox.unwrap_or([0.; 4]);
            let mut end = start + 1;
            while end < kept.len()
                && kept[end].bbox.is_some_and(|b| {
                    (b[1] - first[1]).abs() <= ((first[3] - first[1]) * 0.45).max(5.)
                })
            {
                end += 1;
            }
            kept[start..end].sort_by(|a, b| {
                a.bbox.unwrap_or([0.; 4])[0].total_cmp(&b.bbox.unwrap_or([0.; 4])[0])
            });
            start = end;
        }
        for (i, block) in kept.iter_mut().enumerate() {
            block.id = format!("block-{i}");
        }
        kept
    }
}
fn normalized(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect()
}
fn duplicate(a: &OcrBlock, b: &OcrBlock) -> bool {
    let (Some(x), Some(y)) = (a.bbox, b.bbox) else {
        return false;
    };
    let intersection =
        (x[2].min(y[2]) - x[0].max(y[0])).max(0.) * (x[3].min(y[3]) - x[1].max(y[1])).max(0.);
    let area = |b: [f64; 4]| ((b[2] - b[0]) * (b[3] - b[1])).max(1.);
    if intersection / area(x).min(area(y)) < 0.65 {
        return false;
    }
    let a = normalized(&a.text);
    let b = normalized(&b.text);
    if a == b {
        return true;
    }
    if a.chars().count().min(b.chars().count()) >= 4 && (a.contains(&b) || b.contains(&a)) {
        return true;
    }
    let a: Vec<_> = a.chars().take(512).collect();
    let b: Vec<_> = b.chars().take(512).collect();
    if a.len().min(b.len()) < 4 {
        return false;
    }
    let mut previous: Vec<_> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut current = vec![i + 1; b.len() + 1];
        for (j, cb) in b.iter().enumerate() {
            current[j + 1] = (previous[j] + usize::from(ca != cb))
                .min(previous[j + 1] + 1)
                .min(current[j] + 1);
        }
        previous = current;
    }
    previous[b.len()] as f64 / a.len().max(b.len()) as f64 <= 0.2
}

#[cfg(test)]
mod tests {
    use super::*;
    fn block(text: &str, bbox: [f64; 4], confidence: f32) -> OcrBlock {
        OcrBlock {
            id: "local".into(),
            text: text.into(),
            label: None,
            bbox: Some(bbox),
            confidence: Some(confidence),
            polygon: Some(vec![
                Point {
                    x: bbox[0],
                    y: bbox[1],
                },
                Point {
                    x: bbox[2],
                    y: bbox[1],
                },
                Point {
                    x: bbox[2],
                    y: bbox[3],
                },
                Point {
                    x: bbox[0],
                    y: bbox[3],
                },
            ]),
        }
    }
    #[test]
    fn configuration_semantics_and_legacy_defaults() {
        let mut p: ParallelConfig = serde_json::from_str("{}").unwrap();
        assert!(!p.enable_parallel_inference);
        assert_eq!(p.requested(Device::Cpu), (0, 1));
        assert_eq!(p.requested(Device::Auto), (1, 0));
        p.enable_parallel_inference = true;
        p.gpu_worker_count = 0;
        assert!(p.validate(Device::Auto, 8).is_err());
        p.enable_cpu_workers = true;
        p.cpu_worker_count = 2;
        assert!(p.validate(Device::Auto, 8).is_ok());
        assert!(p.validate(Device::Cuda, 8).is_err());
        assert!(p.validate(Device::Cpu, 1).is_err());
    }
    #[test]
    fn tiling_preserves_coverage_and_detector_budget() {
        let blank = RgbImage::new(1920, 1080);
        assert_eq!(FrameSplitter::plan(&blank, 4, 960).len(), 1);
        let image = RgbImage::from_fn(1920, 1080, |x, y| {
            image::Rgb([if (x / 4 + y / 4) % 2 == 0 { 0 } else { 255 }; 3])
        });
        let tiles = FrameSplitter::plan(&image, 4, 960);
        assert_eq!(tiles.len(), 4);
        for y in (0..1080).step_by(7) {
            for x in (0..1920).step_by(7) {
                assert!(
                    tiles
                        .iter()
                        .any(|t| x >= t.x && x < t.x + t.width && y >= t.y && y < t.y + t.height)
                );
            }
        }
        assert!(tiles.iter().map(|t| t.area()).sum::<f64>() < 1920. * 1080. * 1.3);
        assert!(tiles.iter().all(|t| t.detection_resize <= 576));
        assert_eq!(FrameSplitter::plan(&image, 1, 960).len(), 1);
    }
    #[test]
    fn coordinates_deduplication_and_border_repair() {
        let tile = Tile {
            id: 1,
            x: 500,
            y: 200,
            width: 600,
            height: 600,
            detection_resize: 512,
        };
        let local = block("Health 100", [10., 30., 180., 55.], 0.93);
        assert!(touches_internal_edge(&local, &tile, (1920, 1080)));
        let global = map_block(local, &tile, (1920, 1080)).unwrap();
        assert_eq!(global.bbox, Some([510., 230., 680., 255.]));
        assert_eq!(
            global.polygon.as_ref().unwrap()[0],
            Point { x: 510., y: 230. }
        );
        let merged = ResultMerger::merge(vec![
            global,
            block("Health 100", [512., 230., 682., 255.], 0.8),
            block("Health 100", [800., 230., 970., 255.], 0.9),
        ]);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].confidence, Some(0.93));
        assert!(!touches_internal_edge(
            &block("world", [0., 0., 100., 20.], 1.),
            &Tile::whole(1920, 1080, 960),
            (1920, 1080)
        ));
    }
}

//! Conservative temporal decisions inside FrameCache. Features authorize a
//! recognition-only attempt, never establish that changed glyphs mean old text.
use crate::{
    cache::FrameCache,
    pipeline::{Quad, bbox},
    regions::Region,
};
use gameocr_core::engine::{TemporalConfig, TemporalStrategy};
use image::RgbImage;
use std::time::{Duration, Instant};

pub struct Track {
    pub quad: Quad,
    pub confidence: f32,
    pub stable_detections: u32,
    pub last_recognition: Instant,
    pub reuse_count: u32,
    pub edge_count: usize,
    pub surround_mean: f32,
}
pub struct State {
    pub frame_id: u64,
    pub context: [u64; 3],
    pub last_detection: Instant,
    pub reuse_count: u32,
    pub tracks: Vec<Track>,
}
pub struct Decision {
    pub full: bool,
    pub reason: &'static str,
}
impl Decision {
    fn full(reason: &'static str) -> Self {
        Self { full: true, reason }
    }
}
fn gray(image: &RgbImage, x: u32, y: u32) -> i32 {
    let p = image.get_pixel(x, y);
    (p[0] as i32 * 77 + p[1] as i32 * 150 + p[2] as i32 * 29) >> 8
}
// Signed local high-frequency residual: uniform background illumination cancels.
fn gradient(image: &RgbImage, x: u32, y: u32) -> (i32, i32) {
    let c = gray(image, x, y);
    (
        c - gray(image, x.saturating_sub(1), y),
        c - gray(image, x, y.saturating_sub(1)),
    )
}
fn bounds(q: &Quad, image: &RgbImage) -> [u32; 4] {
    let b = bbox(q);
    [
        b[0].floor().max(0.) as u32,
        b[1].floor().max(0.) as u32,
        (b[2].ceil().max(0.) as u32).min(image.width()),
        (b[3].ceil().max(0.) as u32).min(image.height()),
    ]
}
pub fn assess(
    cache: &FrameCache,
    image: &RgbImage,
    cfg: &TemporalConfig,
    frame_id: u64,
    context: [u64; 3],
    scope: Region,
) -> Decision {
    if cfg.strategy == TemporalStrategy::Full {
        return Decision::full("forced-full");
    }
    let Some(state) = &cache.temporal else {
        return Decision::full("uninitialized");
    };
    if frame_id <= state.frame_id || context != state.context {
        return Decision::full("frame-context");
    }
    if state.last_detection.elapsed() >= Duration::from_millis(cfg.refresh_ms)
        || state.reuse_count >= cfg.max_reuse
    {
        return Decision::full("periodic-refresh");
    }
    if image.dimensions() != cache.image.dimensions() {
        return Decision::full("resize");
    }
    // Two actual full detections must agree before boundaries become trusted.
    if state.tracks.len() != cache.lines.len()
        || state
            .tracks
            .iter()
            .any(|t| t.stable_detections < 2 || t.confidence < cfg.min_confidence)
    {
        return Decision::full("untrusted-boundaries");
    }
    if image.as_raw() == cache.image.as_raw() {
        return Decision {
            full: false,
            reason: "exact-frame",
        };
    }
    if cache.lines.is_empty() {
        return Decision::full("changed-empty-scene");
    }
    let boxes: Vec<_> = cache
        .lines
        .iter()
        .map(|(q, _, _, _)| bounds(q, image))
        .collect();
    // Scan every pixel in scope, including all areas outside old boxes. No
    // thumbnail sampling that could hide a newly appearing HUD digit.
    for y in scope.y..scope.y + scope.h {
        let spans: Vec<_> = boxes.iter().filter(|b| y >= b[1] && y < b[3]).collect();
        for x in scope.x..scope.x + scope.w {
            if image.get_pixel(x, y) == cache.image.get_pixel(x, y) {
                continue;
            }
            let a = gradient(&cache.image, x, y);
            let b = gradient(image, x, y);
            let delta = (a.0 - b.0).abs().max((a.1 - b.1).abs());
            let inside = spans.iter().any(|b| {
                x >= b[0].saturating_add(3)
                    && x + 3 < b[2]
                    && y >= b[1].saturating_add(3)
                    && y + 3 < b[3]
            });
            if !inside && delta > cfg.edge_delta as i32 {
                return Decision::full("new-edge-or-boundary-change");
            }
        }
    }
    if cfg.strategy == TemporalStrategy::CacheOnly
        && cache
            .lines
            .iter()
            .any(|(q, w, _, _)| cache.line(image, q, *w).is_none())
    {
        return Decision::full("cache-only-content-changed");
    }
    // Content equality is tested separately by exact padded line crops; this
    // guard only authorizes running the recognizer on unchanged boundaries.
    Decision {
        full: false,
        reason: "trusted-boundaries",
    }
}

pub fn update(
    previous: Option<&FrameCache>,
    image: &RgbImage,
    lines: &[(Quad, usize, String, f32)],
    full: bool,
    frame_id: u64,
    context: [u64; 3],
    reused: &[bool],
) -> State {
    let now = Instant::now();
    let old = previous
        .and_then(|c| c.temporal.as_ref())
        .filter(|s| s.context == context && frame_id > s.frame_id);
    let tracks = lines
        .iter()
        .enumerate()
        .map(|(index, (quad, _, _, confidence))| {
            let prior = old.and_then(|s| {
                s.tracks.iter().find(|t| {
                    t.quad
                        .iter()
                        .zip(quad)
                        .all(|(a, b)| (a.x - b.x).abs() <= 2. && (a.y - b.y).abs() <= 2.)
                })
            });
            let b = bounds(quad, image);
            let mut edges = 0;
            let mut sum = 0u64;
            let mut count = 0;
            for y in b[1].saturating_sub(4)..(b[3] + 4).min(image.height()) {
                for x in b[0].saturating_sub(4)..(b[2] + 4).min(image.width()) {
                    let g = gradient(image, x, y);
                    edges += usize::from(g.0.abs() + g.1.abs() > 40);
                    if x < b[0] || x >= b[2] || y < b[1] || y >= b[3] {
                        sum += gray(image, x, y) as u64;
                        count += 1;
                    }
                }
            }
            let reuse = reused.get(index) == Some(&true);
            Track {
                quad: *quad,
                confidence: *confidence,
                stable_detections: if full {
                    prior.map_or(1, |t| t.stable_detections.saturating_add(1))
                } else {
                    prior.map_or(0, |t| t.stable_detections)
                },
                last_recognition: if reuse {
                    prior.map_or(now, |t| t.last_recognition)
                } else {
                    now
                },
                reuse_count: if reuse {
                    prior.map_or(0, |t| t.reuse_count.saturating_add(1))
                } else {
                    0
                },
                edge_count: edges,
                surround_mean: sum as f32 / (count.max(1) as f32),
            }
        })
        .collect();
    State {
        frame_id,
        context,
        last_detection: if full {
            now
        } else {
            old.map_or(now, |s| s.last_detection)
        },
        reuse_count: if full {
            0
        } else {
            old.map_or(0, |s| s.reuse_count + 1)
        },
        tracks,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gameocr_core::types::Point;
    use image::Rgb;
    fn cache() -> FrameCache {
        let q = [
            Point { x: 10., y: 10. },
            Point { x: 50., y: 10. },
            Point { x: 50., y: 30. },
            Point { x: 10., y: 30. },
        ];
        let image = RgbImage::from_pixel(100, 60, Rgb([20, 20, 20]));
        let lines = vec![(q, 320, "100".into(), 0.99)];
        let mut s = update(None, &image, &lines, true, 1, [1, 1, 1], &[]);
        s.tracks[0].stable_detections = 2;
        FrameCache {
            image,
            lines,
            regions: vec![],
            refreshed: Instant::now(),
            frames: 0,
            edge: 960,
            temporal: Some(s),
        }
    }
    #[test]
    fn changes_never_reuse_changed_glyphs_and_new_edges_force_detection() {
        let c = cache();
        let cfg = TemporalConfig::default();
        let r = Region {
            x: 0,
            y: 0,
            w: 100,
            h: 60,
        };
        let mut im = c.image.clone();
        assert!(!assess(&c, &im, &cfg, 2, [1, 1, 1], r).full);
        im.put_pixel(25, 20, Rgb([255, 255, 255]));
        assert!(!assess(&c, &im, &cfg, 2, [1, 1, 1], r).full);
        assert!(c.line(&im, &c.lines[0].0, 320).is_none());
        im.put_pixel(75, 40, Rgb([255, 255, 255]));
        assert!(assess(&c, &im, &cfg, 2, [1, 1, 1], r).full);
        assert!(assess(&c, &c.image, &cfg, 1, [1, 1, 1], r).full);
        assert!(assess(&c, &c.image, &cfg, 2, [2, 1, 1], r).full);
    }
    #[test]
    fn boundaries_timeout_and_low_confidence_are_not_trusted() {
        let mut c = cache();
        let cfg = TemporalConfig::default();
        let r = Region {
            x: 0,
            y: 0,
            w: 100,
            h: 60,
        };
        let mut im = c.image.clone();
        im.put_pixel(49, 20, Rgb([255, 255, 255]));
        assert!(assess(&c, &im, &cfg, 2, [1, 1, 1], r).full);
        c.temporal.as_mut().unwrap().last_detection = Instant::now() - Duration::from_secs(2);
        assert!(assess(&c, &c.image, &cfg, 2, [1, 1, 1], r).full);
        c.temporal.as_mut().unwrap().last_detection = Instant::now();
        c.temporal.as_mut().unwrap().tracks[0].confidence = 0.4;
        assert!(assess(&c, &c.image, &cfg, 2, [1, 1, 1], r).full);
    }
}

use super::FEATURE_VERSION;
use crate::parallel::quiet_seam;
use image::RgbImage;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrameFeatures {
    pub schema: u32,
    pub width: u32,
    pub height: u32,
    pub hash: String,
    pub histogram: [f64; 16],
    pub edge_grid: [f64; 16],
    pub text_grid: [f64; 16],
    pub contrast_grid: [f64; 16],
    pub gray_grid: [f64; 16],
    pub edge_density: f64,
    pub gradient: f64,
    pub variance: f64,
    pub contrast: f64,
    pub text_density: f64,
    pub components: u32,
    pub active_cells: u32,
    pub dispersion: f64,
    pub seam_energy: f64,
    pub seam_text: f64,
    pub crossing_regions: u32,
    pub imbalance: f64,
    pub horizontal_projection: f64,
    pub vertical_projection: f64,
    pub change_ratio: Option<f64>,
}

#[derive(Default)]
pub struct FrameAnalyzer {
    previous: Option<(u32, u32, Vec<u8>)>,
}
impl FrameAnalyzer {
    pub fn reset(&mut self) {
        self.previous = None;
    }

    pub fn analyze(&mut self, image: &RgbImage) -> FrameFeatures {
        let (sw, sh) = image.dimensions();
        let scale = 320. / sw.max(sh).max(1) as f64;
        let w = ((sw as f64 * scale.min(1.)).round() as usize).max(1);
        let h = ((sh as f64 * scale.min(1.)).round() as usize).max(1);
        let n = w * h;
        // Four samples per thumbnail pixel; no full-resolution RGB copy/resize.
        let mut gray = vec![0u8; n];
        if sw > 0 && sh > 0 {
            for y in 0..h {
                for x in 0..w {
                    let mut sum = 0u32;
                    for (dx, dy) in [(1, 1), (3, 1), (1, 3), (3, 3)] {
                        let px =
                            (((4 * x + dx) as u64 * sw as u64 / (4 * w) as u64) as u32).min(sw - 1);
                        let py =
                            (((4 * y + dy) as u64 * sh as u64 / (4 * h) as u64) as u32).min(sh - 1);
                        let p = image.get_pixel(px, py);
                        sum += (77 * p[0] as u32 + 150 * p[1] as u32 + 29 * p[2] as u32) >> 8;
                    }
                    gray[y * w + x] = (sum / 4) as u8;
                }
            }
        }
        let change = self
            .previous
            .as_ref()
            .filter(|(pw, ph, p)| *pw == sw && *ph == sh && p.len() == n)
            .map(|(_, _, p)| {
                p.iter()
                    .zip(&gray)
                    .filter(|(a, b)| a.abs_diff(**b) > 12)
                    .count() as f64
                    / n as f64
            });
        let mut hash = 0u64;
        for y in 0..8 {
            for x in 0..8 {
                let row = ((2 * y + 1) * h / 16).min(h - 1) * w;
                let a = ((2 * x + 1) * w / 18).min(w - 1);
                let b = ((2 * x + 3) * w / 18).min(w - 1);
                hash = (hash << 1) | u64::from(gray[row + a] > gray[row + b]);
            }
        }
        let mut f = FrameFeatures {
            schema: FEATURE_VERSION,
            width: sw,
            height: sh,
            hash: format!("{hash:016x}"),
            histogram: [0.; 16],
            edge_grid: [0.; 16],
            text_grid: [0.; 16],
            contrast_grid: [0.; 16],
            gray_grid: [0.; 16],
            edge_density: 0.,
            gradient: 0.,
            variance: 0.,
            contrast: 0.,
            text_density: 0.,
            components: 0,
            active_cells: 0,
            dispersion: 0.,
            seam_energy: 0.,
            seam_text: 0.,
            crossing_regions: 0,
            imbalance: 0.,
            horizontal_projection: 0.,
            vertical_projection: 0.,
            change_ratio: change,
        };
        let mut counts = [0f64; 16];
        let mut integral = vec![0u32; (w + 1) * (h + 1)];
        let mut mean = 0.;
        let mut square = 0.;
        for y in 0..h {
            let mut row = 0;
            for x in 0..w {
                let p = gray[y * w + x] as u32;
                row += p;
                integral[(y + 1) * (w + 1) + x + 1] = integral[y * (w + 1) + x + 1] + row;
                f.histogram[(p / 16) as usize] += 1. / n as f64;
                mean += p as f64 / n as f64;
                square += (p * p) as f64 / n as f64;
            }
        }
        f.variance = ((square - mean * mean) / 16256.25).clamp(0., 1.);
        let sx = if sw >= 4 {
            quiet_seam(image, true) as f64 / sw as f64
        } else {
            0.5
        };
        let sy = if sh >= 4 {
            quiet_seam(image, false) as f64 / sh as f64
        } else {
            0.5
        };
        let cutx = (sx * w as f64) as usize;
        let cuty = (sy * h as f64) as usize;
        let mut mask = vec![false; n];
        let mut seam_pixels = 0f64;
        for y in 0..h {
            for x in 0..w {
                let i = y * w + x;
                let cell = (y * 4 / h).min(3) * 4 + (x * 4 / w).min(3);
                counts[cell] += 1.;
                f.gray_grid[cell] += gray[i] as f64 / 255.;
                let (x0, y0, x1, y1) = (
                    x.saturating_sub(4),
                    y.saturating_sub(4),
                    (x + 5).min(w),
                    (y + 5).min(h),
                );
                let local = (integral[y1 * (w + 1) + x1] + integral[y0 * (w + 1) + x0]
                    - integral[y0 * (w + 1) + x1]
                    - integral[y1 * (w + 1) + x0]) as f64
                    / ((x1 - x0) * (y1 - y0)) as f64;
                let contrast = (gray[i] as f64 - local).abs() / 255.;
                f.contrast += contrast / n as f64;
                if contrast > 0.06 {
                    f.contrast_grid[cell] += 1.;
                }
                let xm = x.saturating_sub(1);
                let xp = (x + 1).min(w - 1);
                let ym = y.saturating_sub(1);
                let yp = (y + 1).min(h - 1);
                let p = |a, b| gray[b * w + a] as f64;
                let gx =
                    (p(xp, ym) + 2. * p(xp, y) + p(xp, yp) - p(xm, ym) - 2. * p(xm, y) - p(xm, yp))
                        .abs();
                let gy =
                    (p(xm, yp) + 2. * p(x, yp) + p(xp, yp) - p(xm, ym) - 2. * p(x, ym) - p(xp, ym))
                        .abs();
                let energy = ((gx + gy) / 2040.).min(1.);
                f.gradient += energy / n as f64;
                if energy > 0.075 {
                    f.edge_grid[cell] += 1.;
                    f.edge_density += 1. / n as f64;
                }
                // Both polarities; threshold is only a workload hint, never a text mask for OCR.
                mask[i] = contrast > 0.045 && energy > 0.025;
                if x.abs_diff(cutx) <= 2 || y.abs_diff(cuty) <= 2 {
                    f.seam_energy += energy;
                    seam_pixels += 1.;
                }
            }
        }
        f.seam_energy /= seam_pixels.max(1.);
        let mut seen = vec![false; n];
        let mut stack = Vec::new();
        let mut rows = vec![0u32; h];
        let mut cols = vec![0u32; w];
        let mut seam_text = 0.;
        let mut text_pixels = 0.;
        for seed in 0..n {
            if seen[seed] || !mask[seed] {
                continue;
            }
            seen[seed] = true;
            stack.clear();
            stack.push(seed);
            let mut cursor = 0;
            let (mut minx, mut miny, mut maxx, mut maxy) = (w, h, 0, 0);
            while cursor < stack.len() {
                let i = stack[cursor];
                cursor += 1;
                let x = i % w;
                let y = i / w;
                minx = minx.min(x);
                maxx = maxx.max(x);
                miny = miny.min(y);
                maxy = maxy.max(y);
                for j in [
                    if x > 0 { i - 1 } else { i },
                    if x + 1 < w { i + 1 } else { i },
                    if y > 0 { i - w } else { i },
                    if y + 1 < h { i + w } else { i },
                ] {
                    if !seen[j] && mask[j] {
                        seen[j] = true;
                        stack.push(j);
                    }
                }
            }
            let bw = maxx - minx + 1;
            let bh = maxy - miny + 1;
            let area = bw * bh;
            // Reject isolated noise and huge textured components, allow horizontal AND vertical geometry.
            if stack.len() < 3
                || area > n / 5
                || bw.max(bh) > w.max(h) * 3 / 4
                || stack.len() * 20 < area
            {
                continue;
            }
            f.components += 1;
            if (minx < cutx && maxx > cutx && bw > bh * 3)
                || (miny < cuty && maxy > cuty && bh > bw * 3)
            {
                f.crossing_regions += 1;
            }
            for &i in &stack {
                let x = i % w;
                let y = i / w;
                f.text_grid[(y * 4 / h).min(3) * 4 + (x * 4 / w).min(3)] += 1.;
                rows[y] += 1;
                cols[x] += 1;
                text_pixels += 1.;
                if x.abs_diff(cutx) <= 4 || y.abs_diff(cuty) <= 4 {
                    seam_text += 1.;
                }
            }
        }
        f.text_density = text_pixels / n as f64;
        f.seam_text = seam_text / text_pixels.max(1.);
        f.horizontal_projection =
            rows.iter().filter(|&&r| r as f64 / w as f64 > 0.03).count() as f64 / h as f64;
        f.vertical_projection =
            cols.iter().filter(|&&c| c as f64 / h as f64 > 0.03).count() as f64 / w as f64;
        let mut quadrant = [0.; 4];
        let mut total = 0.;
        for (i, count) in counts.iter().enumerate() {
            f.edge_grid[i] /= count.max(1.);
            f.text_grid[i] /= count.max(1.);
            f.contrast_grid[i] /= count.max(1.);
            f.gray_grid[i] /= count.max(1.);
            if f.text_grid[i] > 0.005 && f.edge_grid[i] > 0.008 {
                f.active_cells += 1;
            }
            let value = f.text_grid[i] + f.edge_grid[i] * 0.25;
            total += value;
            quadrant[(i / 4 >= 2) as usize * 2 + (i % 4 >= 2) as usize] += value;
        }
        if total > 0. {
            f.dispersion = quadrant
                .iter()
                .map(|v| {
                    let p = v / total;
                    if p > 0. { -p * p.ln() / 4f64.ln() } else { 0. }
                })
                .sum();
            f.imbalance = (quadrant.iter().copied().fold(0., f64::max)
                - quadrant.iter().copied().fold(f64::INFINITY, f64::min))
                / total;
        }
        self.previous = Some((sw, sh, gray));
        f
    }
}

impl FrameFeatures {
    pub fn valid(&self) -> bool {
        self.schema == FEATURE_VERSION
            && self.width > 0
            && self.height > 0
            && self.hash.len() == 16
            && u64::from_str_radix(&self.hash, 16).is_ok()
            && self
                .vector()
                .iter()
                .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
            && self
                .histogram
                .iter()
                .chain(&self.gray_grid)
                .all(|v| v.is_finite() && (0.0..=1.00001).contains(v))
    }
    pub fn vector(&self) -> Vec<f64> {
        let mut v = vec![
            (self.width as f64 / self.height.max(1) as f64 / 4.).min(1.),
            ((self.width as f64 * self.height as f64).max(1.).log2() / 26.).clamp(0., 1.),
            self.edge_density,
            self.gradient,
            self.variance,
            self.contrast,
            self.text_density,
            (self.components as f64 / 200.).min(1.),
            self.active_cells as f64 / 16.,
            self.dispersion,
            self.seam_energy,
            self.seam_text,
            (self.crossing_regions as f64 / 8.).min(1.),
            self.imbalance,
            self.horizontal_projection,
            self.vertical_projection,
        ];
        v.extend(self.edge_grid);
        v.extend(self.text_grid);
        v.extend(self.contrast_grid);
        v
    }
    pub fn distance(&self, other: &Self) -> f64 {
        if !self.valid() || !other.valid() {
            return f64::INFINITY;
        }
        let a = self.vector();
        let b = other.vector();
        let mut sum = 0.;
        let mut weight = 0.;
        for (i, (x, y)) in a.iter().zip(b).enumerate() {
            let k = match i {
                0 | 1 => 8.,
                7..=15 => 3.,
                _ => 1.,
            };
            sum += k * (x - y).powi(2);
            weight += k;
        }
        (sum / weight).sqrt()
    }
    pub fn near_image(&self, other: &Self) -> bool {
        let (Ok(a), Ok(b)) = (
            u64::from_str_radix(&self.hash, 16),
            u64::from_str_radix(&other.hash, 16),
        ) else {
            return false;
        };
        let area = self.width as f64 * self.height as f64
            / (other.width as f64 * other.height as f64).max(1.);
        (a ^ b).count_ones() <= 6
            && (0.9..=1.1).contains(&area)
            && self.distance(other) < 0.055
            && self
                .histogram
                .iter()
                .zip(other.histogram)
                .map(|(a, b)| (a - b).abs())
                .sum::<f64>()
                < 0.3
    }
}

pub struct FallbackRiskEstimator;
impl FallbackRiskEstimator {
    pub fn estimate(f: &FrameFeatures, historical: Option<f64>) -> f64 {
        let visual = if f.components < 3 || f.dispersion < 0.45 {
            0.85
        } else {
            (f.seam_text * 3.5
                + f.seam_energy * 2.
                + (f.crossing_regions as f64 * 0.15).min(0.6)
                + f.imbalance * 0.15)
                .clamp(0., 1.)
        };
        historical.map_or(visual, |h| (visual * 0.4 + h * 0.6).clamp(0., 1.))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn layout(spread: bool) -> RgbImage {
        let mut image = RgbImage::new(1920, 1080);
        for (x, y) in if spread {
            vec![(60, 60), (1450, 60), (60, 900), (1450, 900)]
        } else {
            vec![(600, 900)]
        } {
            for k in 0..16 {
                for dy in 0..28 {
                    for dx in 0..10 {
                        image.put_pixel(x + k * 17 + dx, y + dy, image::Rgb([230; 3]));
                    }
                }
            }
        }
        image
    }
    #[test]
    fn stable_scaled_and_spatial_features() {
        let image = layout(true);
        let mut analyzer = FrameAnalyzer::default();
        let a = analyzer.analyze(&image);
        let b = analyzer.analyze(&image);
        assert!(a.valid());
        assert_eq!(a.vector(), b.vector());
        assert_eq!(b.change_ratio, Some(0.));
        assert!(a.near_image(&b));
        let smaller =
            image::imageops::resize(&image, 1280, 720, image::imageops::FilterType::Triangle);
        let scaled = analyzer.analyze(&smaller);
        assert!(a.distance(&scaled) < 0.12);
        let dialogue = analyzer.analyze(&layout(false));
        assert!(a.dispersion > dialogue.dispersion + 0.3);
        assert!(a.active_cells > dialogue.active_cells);
        assert!(FallbackRiskEstimator::estimate(&dialogue, None) > 0.7);
    }
    #[test]
    fn blank_tiny_and_noise_are_bounded() {
        for (w, h) in [(0, 0), (1, 1), (1, 3000), (3840, 2160), (40, 20)] {
            let f = FrameAnalyzer::default().analyze(&RgbImage::new(w, h));
            assert_eq!(f.components, 0);
            assert_eq!(f.edge_density, 0.);
            assert!(f.vector().iter().all(|x| x.is_finite()));
        }
        let noise = RgbImage::from_fn(640, 360, |x, y| {
            image::Rgb([((x.wrapping_mul(1664525) ^ y.wrapping_mul(1013904223)) >> 8) as u8; 3])
        });
        assert!(FrameAnalyzer::default().analyze(&noise).valid());
    }
}

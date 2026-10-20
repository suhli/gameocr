//! Bounded proposals for missed small text. Confidence is deliberately absent:
//! confidently recognized lines say nothing about text the detector omitted.
use crate::{
    pipeline::{Quad, bbox},
    regions::{self, Region},
};
use anyhow::Result;
use gameocr_core::engine::OnnxConfig;
use image::RgbImage;

pub fn select(
    image: &RgbImage,
    config: &OnnxConfig,
    edge: u32,
    overview: &[Quad],
) -> Result<Vec<Region>> {
    let mut full = config.clone();
    full.preserve_small_text = true;
    let plan = regions::plan(image.dimensions(), &full, edge)?;
    let scope = plan[0];
    let ratio = (edge as f64 / scope.w.max(scope.h) as f64).min(1.);
    // Well-resolved detection boxes can explain local edges. Tiny boxes cannot
    // explain neighboring missing characters, so retain that visual evidence.
    let covered: Vec<_> = overview
        .iter()
        .map(bbox)
        .filter(|b| (b[3] - b[1]) * ratio >= 14.)
        .collect();
    let mut scores: Vec<_> = plan
        .into_iter()
        .skip(1)
        .filter_map(|r| {
            let score = unexplained_detail(image, r, &covered);
            (score > 0.001).then_some((r, score))
        })
        .collect();
    scores.sort_by(|a, b| b.1.total_cmp(&a.1));
    scores.truncate(config.refinement_regions);
    Ok(scores.into_iter().map(|(r, _)| r).collect())
}
fn unexplained_detail(image: &RgbImage, r: Region, covered: &[[f64; 4]]) -> f64 {
    // Scan 32px cells. Edge occupancy rejects flat cells and very dense noise;
    // it is a proposal heuristic, never a completeness or correctness guarantee.
    let mut score = 0.;
    for y0 in (r.y..r.y + r.h).step_by(32) {
        for x0 in (r.x..r.x + r.w).step_by(32) {
            let x1 = (x0 + 32).min(r.x + r.w);
            let y1 = (y0 + 32).min(r.y + r.h);
            if covered.iter().any(|b| {
                b[0] <= x0 as f64 && b[1] <= y0 as f64 && b[2] >= x1 as f64 && b[3] >= y1 as f64
            }) {
                continue;
            }
            let mut edges = 0u32;
            let mut count = 0u32;
            for y in (y0 + 1..y1).step_by(2) {
                for x in (x0 + 1..x1).step_by(2) {
                    let lum = |px: u32, py: u32| {
                        let p = image.get_pixel(px, py);
                        (p[0] as i32 * 77 + p[1] as i32 * 150 + p[2] as i32 * 29) >> 8
                    };
                    let v = lum(x, y);
                    let dx = (v - lum(x - 1, y)).abs();
                    let dy = (v - lum(x, y - 1)).abs();
                    edges += u32::from(dx + dy >= 60);
                    count += 1;
                }
            }
            let occupancy = edges as f64 / count.max(1) as f64;
            if (0.015..=0.4).contains(&occupancy) {
                score += edges as f64;
            }
        }
    }
    score / (r.w as f64 * r.h as f64)
}
#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgb;
    #[test]
    fn flat_background_has_no_proposals_and_text_strokes_obey_budget() {
        let mut image = RgbImage::new(3840, 2160);
        let c = OnnxConfig {
            selective_detection: true,
            refinement_regions: 2,
            ..Default::default()
        };
        assert!(select(&image, &c, 960, &[]).unwrap().is_empty());
        for y in 100..125 {
            for x in 100..600 {
                if x % 9 < 3 {
                    image.put_pixel(x, y, Rgb([255, 255, 255]));
                }
            }
        }
        let selected = select(&image, &c, 960, &[]).unwrap();
        assert!(!selected.is_empty());
        assert!(selected.len() <= 2);
        assert!(
            selected
                .iter()
                .any(|r| r.x <= 100 && r.y <= 100 && r.x + r.w >= 600 && r.y + r.h >= 125)
        );
    }
}

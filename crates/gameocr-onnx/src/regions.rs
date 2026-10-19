//! Bounded, overlapping source-resolution detection with original-frame quads.
use crate::pipeline::{Quad, bbox};
use anyhow::{Result, ensure};
use gameocr_core::{engine::OnnxConfig, types::Point};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Region {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}
impl Region {
    pub fn scope(source: (u32, u32), roi: Option<[f64; 4]>) -> Self {
        let [x, y, w, h] = roi.unwrap_or([0., 0., 1., 1.]);
        let left = (x * source.0 as f64).floor() as u32;
        let top = (y * source.1 as f64).floor() as u32;
        let right = ((x + w) * source.0 as f64).ceil().min(source.0 as f64) as u32;
        let bottom = ((y + h) * source.1 as f64).ceil().min(source.1 as f64) as u32;
        Self {
            x: left,
            y: top,
            w: right - left,
            h: bottom - top,
        }
    }
    pub fn restore(self, q: Quad) -> Quad {
        q.map(|p| Point {
            x: p.x + self.x as f64,
            y: p.y + self.y as f64,
        })
    }
    pub fn clipped(self, q: &Quad, scope: Self) -> bool {
        let b = bbox(q);
        (self.x > scope.x && b[0] <= self.x as f64 + 3.)
            || (self.y > scope.y && b[1] <= self.y as f64 + 3.)
            || (self.x + self.w < scope.x + scope.w && b[2] >= (self.x + self.w) as f64 - 4.)
            || (self.y + self.h < scope.y + scope.h && b[3] >= (self.y + self.h) as f64 - 4.)
    }
}
pub fn plan(source: (u32, u32), config: &OnnxConfig, edge: u32) -> Result<Vec<Region>> {
    let scope = Region::scope(source, config.detection_region);
    ensure!(scope.w > 0 && scope.h > 0, "Empty detection region");
    let mut regions = vec![scope]; // Overview preserves long lines crossing tiles.
    if !config.preserve_small_text || scope.w.max(scope.h) <= edge {
        return Ok(regions);
    }
    let halo = (edge / 8).min(128);
    let core = edge - 2 * halo;
    let nx = scope.w.div_ceil(core);
    let ny = scope.h.div_ceil(core);
    ensure!(
        nx as u64 * ny as u64 <= 64,
        "Small-text detection exceeds 64 regions; select a smaller ROI or increase detection edge"
    );
    for y in 0..ny {
        for x in 0..nx {
            // Balanced cores avoid a narrow final tile; halo never leaves the ROI.
            let left = (x * scope.w / nx).saturating_sub(halo);
            let top = (y * scope.h / ny).saturating_sub(halo);
            let right = (((x + 1) * scope.w / nx) + halo).min(scope.w);
            let bottom = (((y + 1) * scope.h / ny) + halo).min(scope.h);
            regions.push(Region {
                x: scope.x + left,
                y: scope.y + top,
                w: right - left,
                h: bottom - top,
            });
        }
    }
    Ok(regions)
}
#[derive(Clone)]
pub struct Candidate {
    pub quad: Quad,
    pub clipped: bool,
    pub region: usize,
}
fn area(b: [f64; 4]) -> f64 {
    (b[2] - b[0]).max(0.) * (b[3] - b[1]).max(0.)
}
fn overlap(a: [f64; 4], b: [f64; 4]) -> f64 {
    area([
        a[0].max(b[0]),
        a[1].max(b[1]),
        a[2].min(b[2]),
        a[3].min(b[3]),
    ])
}
pub fn merge(mut candidates: Vec<Candidate>) -> Vec<Quad> {
    // Prefer a complete line to its tile-edge fragments. Do not suppress small
    // neighboring rows just because an overview box encloses several of them.
    candidates.sort_by(|a, b| {
        a.clipped
            .cmp(&b.clipped)
            .then(area(bbox(&b.quad)).total_cmp(&area(bbox(&a.quad))))
    });
    let mut kept: Vec<Candidate> = Vec::new();
    for c in candidates {
        let b = bbox(&c.quad);
        let duplicate = kept.iter().any(|k| {
            let a = bbox(&k.quad);
            let heights = (a[3] - a[1]).min(b[3] - b[1]) / (a[3] - a[1]).max(b[3] - b[1]).max(1.);
            k.region != c.region && heights > 0.5 && overlap(a, b) / area(b).max(1.) > 0.85
        });
        if !duplicate {
            kept.push(c);
        }
    }
    // Join only overlapping, aligned horizontal fragments from different
    // regions when at least one touches an internal edge. Keep tilted quads.
    let mut i = 0;
    while i < kept.len() {
        let mut j = i + 1;
        while j < kept.len() {
            let a = bbox(&kept[i].quad);
            let b = bbox(&kept[j].quad);
            let h1 = a[3] - a[1];
            let h2 = b[3] - b[1];
            let horizontal =
                |q: &Quad| (q[0].y - q[1].y).abs() < 3. && (q[2].y - q[3].y).abs() < 3.;
            if kept[i].region != kept[j].region
                && (kept[i].clipped || kept[j].clipped)
                && horizontal(&kept[i].quad)
                && horizontal(&kept[j].quad)
                && (a[3].min(b[3]) - a[1].max(b[1])) / h1.max(h2).max(1.) > 0.8
                && a[0].max(b[0]) < a[2].min(b[2])
            {
                let [l, t, r, d] = [
                    a[0].min(b[0]),
                    a[1].min(b[1]),
                    a[2].max(b[2]),
                    a[3].max(b[3]),
                ];
                kept[i].quad = [
                    Point { x: l, y: t },
                    Point { x: r, y: t },
                    Point { x: r, y: d },
                    Point { x: l, y: d },
                ];
                kept.remove(j);
                j = i + 1;
            } else {
                j += 1;
            }
        }
        i += 1;
    }
    let mut result: Vec<_> = kept.into_iter().map(|c| c.quad).collect();
    crate::pipeline::reading_order(&mut result);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn four_k_coverage_and_budget() {
        let mut c = OnnxConfig {
            preserve_small_text: true,
            ..Default::default()
        };
        let plan = plan((3840, 2160), &c, 960).unwrap();
        assert_eq!(plan.len(), 19);
        for r in &plan[1..] {
            assert!(r.w <= 960 && r.h <= 960);
        }
        for y in (0..2160).step_by(17) {
            for x in (0..3840).step_by(17) {
                assert!(
                    plan[1..]
                        .iter()
                        .any(|r| x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h)
                );
            }
        }
        c.detection_region = Some([0.5, 0.5, 0.5, 0.5]);
        assert_eq!(
            Region::scope((3840, 2160), c.detection_region),
            Region {
                x: 1920,
                y: 1080,
                w: 1920,
                h: 1080
            }
        );
        c.detection_region = None;
        assert!(super::plan((16384, 16384), &c, 960).is_err());
    }
    fn q(x: f64, y: f64, w: f64) -> Quad {
        [
            Point { x, y },
            Point { x: x + w, y },
            Point {
                x: x + w,
                y: y + 20.,
            },
            Point { x, y: y + 20. },
        ]
    }
    #[test]
    fn seam_duplicates_and_neighboring_rows() {
        let result = merge(vec![
            Candidate {
                quad: q(20., 10., 600.),
                clipped: false,
                region: 0,
            },
            Candidate {
                quad: q(20., 10., 300.),
                clipped: true,
                region: 1,
            },
            Candidate {
                quad: q(300., 10., 320.),
                clipped: true,
                region: 2,
            },
            Candidate {
                quad: q(20., 40., 300.),
                clipped: false,
                region: 1,
            },
        ]);
        assert_eq!(result.len(), 2);
        assert_eq!(bbox(&result[0]), [20., 10., 620., 30.]);
        let stitched = merge(vec![
            Candidate {
                quad: q(20., 10., 300.),
                clipped: true,
                region: 1,
            },
            Candidate {
                quad: q(280., 10., 320.),
                clipped: true,
                region: 2,
            },
        ]);
        assert_eq!(stitched.len(), 1);
        assert_eq!(bbox(&stitched[0])[2], 600.);
    }
}

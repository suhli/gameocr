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
    // An aggressively downscaled overview can split words or span adjacent
    // rows. Merge source-resolution evidence first, then use the overview only
    // where it adds coverage (especially long lines crossing several tiles).
    let overviews: Vec<_> = candidates
        .iter()
        .filter(|c| c.region == 0)
        .cloned()
        .collect();
    candidates.retain(|c| c.region != 0);
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
            let vertical = (a[3].min(b[3]) - a[1].max(b[1])).max(0.);
            let horizontal = (a[2].min(b[2]) - a[0].max(b[0])).max(0.);
            k.region != c.region
                && heights > 0.5
                && horizontal / (b[2] - b[0]).max(1.) > 0.85
                && vertical / (a[3] - a[1]).min(b[3] - b[1]).max(1.) > 0.85
                && (a[1] + a[3] - b[1] - b[3]).abs() < (a[3] - a[1]).max(b[3] - b[1]) * 0.7
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
            let horizontal = |q: &Quad| {
                (q[0].y - q[1].y).abs() <= crate::pipeline::distance(q[0], q[1]) * 0.03 + 1.
                    && (q[2].y - q[3].y).abs() <= crate::pipeline::distance(q[2], q[3]) * 0.03 + 1.
            };
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
    for coarse in overviews {
        let a = bbox(&coarse.quad);
        let matches: Vec<_> = kept
            .iter()
            .enumerate()
            .filter_map(|(index, fine)| {
                let b = bbox(&fine.quad);
                (overlap(a, b) / area(a).min(area(b)).max(1.) > 0.5).then_some(index)
            })
            .collect();
        if matches.is_empty() {
            kept.push(coarse);
        } else {
            let left = matches
                .iter()
                .map(|&i| bbox(&kept[i].quad)[0])
                .fold(f64::INFINITY, f64::min);
            let right = matches
                .iter()
                .map(|&i| bbox(&kept[i].quad)[2])
                .fold(f64::NEG_INFINITY, f64::max);
            let one_line = matches.iter().all(|&i| {
                let b = bbox(&kept[i].quad);
                (a[1] + a[3] - b[1] - b[3]).abs() < (b[3] - b[1]) * 0.6
            });
            if one_line && a[2] - a[0] > (right - left) * 1.15 {
                // The overview reaches beyond all local fragments; retain its
                // complete crop rather than discard characters at a seam.
                for i in matches.into_iter().rev() {
                    kept.remove(i);
                }
                kept.push(coarse);
            }
        }
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
    #[test]
    fn coarse_partial_words_and_double_height_boxes_do_not_duplicate_fine_lines() {
        let mut coarse = q(1969., 104., 69.);
        for p in &mut coarse[2..] {
            p.y = 128.;
        }
        let fine = q(1971., 107., 147.);
        assert_eq!(
            merge(vec![
                Candidate {
                    quad: coarse,
                    clipped: false,
                    region: 0
                },
                Candidate {
                    quad: fine,
                    clipped: false,
                    region: 1
                }
            ]),
            vec![fine]
        );
        let mut coarse = q(1981., 1635., 210.);
        for p in &mut coarse[2..] {
            p.y = 1688.;
        }
        let fine = q(1959., 1653., 223.);
        assert_eq!(
            merge(vec![
                Candidate {
                    quad: coarse,
                    clipped: false,
                    region: 0
                },
                Candidate {
                    quad: fine,
                    clipped: false,
                    region: 1
                }
            ]),
            vec![fine]
        );
    }
    #[test]
    fn tilted_seam_and_different_padding_do_not_leave_fragments() {
        let a = [
            Point { x: 353., y: 649. },
            Point { x: 759., y: 651. },
            Point { x: 759., y: 681. },
            Point { x: 350., y: 678. },
        ];
        let b = [
            Point { x: 520., y: 650. },
            Point { x: 1065., y: 652. },
            Point { x: 1062., y: 682. },
            Point { x: 520., y: 681. },
        ];
        let result = merge(vec![
            Candidate {
                quad: a,
                clipped: true,
                region: 1,
            },
            Candidate {
                quad: b,
                clipped: true,
                region: 2,
            },
        ]);
        assert_eq!(result.len(), 1);
        assert_eq!(bbox(&result[0]), [350., 649., 1065., 682.]);
        let small = [
            Point { x: 1969., y: 141. },
            Point { x: 2039., y: 141. },
            Point { x: 2039., y: 164. },
            Point { x: 1969., y: 164. },
        ];
        let full = [
            Point { x: 1972., y: 144. },
            Point { x: 2117., y: 144. },
            Point { x: 2117., y: 160. },
            Point { x: 1972., y: 160. },
        ];
        assert_eq!(
            merge(vec![
                Candidate {
                    quad: small,
                    clipped: true,
                    region: 1
                },
                Candidate {
                    quad: full,
                    clipped: false,
                    region: 2
                }
            ]),
            vec![full]
        );
    }
}

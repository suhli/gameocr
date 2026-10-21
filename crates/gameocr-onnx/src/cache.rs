//! Exact-pixel reuse: hashes or recognition confidence never prove equivalence.
use crate::{
    pipeline::{Quad, bbox},
    regions::Region,
};
use image::RgbImage;
use std::time::{Duration, Instant};

pub struct FrameCache {
    pub image: RgbImage,
    pub regions: Vec<(Region, Vec<Quad>)>,
    pub lines: Vec<(Quad, usize, String, f32)>,
    pub refreshed: Instant,
    pub frames: u32,
    pub edge: u32,
    pub temporal: Option<crate::temporal::State>,
}
impl FrameCache {
    pub fn valid(&self, image: &RgbImage, edge: u32) -> bool {
        self.image.dimensions() == image.dimensions()
            && self.edge == edge
            && self.frames < 30
            && self.refreshed.elapsed() < Duration::from_secs(10)
    }
    pub fn region(&self, image: &RgbImage, region: Region) -> Option<Vec<Quad>> {
        let entry = self.regions.iter().find(|(r, _)| *r == region)?;
        equal_pixels(&self.image, image, region).then(|| entry.1.clone())
    }
    pub fn line(&self, image: &RgbImage, quad: &Quad, width: usize) -> Option<(String, f32)> {
        let (_, _, text, confidence) = self
            .lines
            .iter()
            .find(|(q, w, _, _)| q == quad && *w == width)?;
        let b = bbox(quad);
        // Include surrounding pixels so changing decorations/adjacent glyphs
        // invalidate a line even when the interior happens to stay identical.
        let x = (b[0].floor() as u32).saturating_sub(16);
        let y = (b[1].floor() as u32).saturating_sub(16);
        let right = (b[2].ceil() as u32 + 17).min(image.width());
        let bottom = (b[3].ceil() as u32 + 17).min(image.height());
        equal_pixels(
            &self.image,
            image,
            Region {
                x,
                y,
                w: right - x,
                h: bottom - y,
            },
        )
        .then(|| (text.clone(), *confidence))
    }
}
pub fn equal_pixels(a: &RgbImage, b: &RgbImage, r: Region) -> bool {
    if a.dimensions() != b.dimensions() || r.x + r.w > a.width() || r.y + r.h > a.height() {
        return false;
    }
    let stride = a.width() as usize * 3;
    (r.y..r.y + r.h).all(|y| {
        let start = y as usize * stride + r.x as usize * 3;
        let end = start + r.w as usize * 3;
        a.as_raw()[start..end] == b.as_raw()[start..end]
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgb;
    #[test]
    fn one_pixel_changes_and_outside_regions() {
        let a = RgbImage::new(64, 64);
        let mut b = a.clone();
        let r = Region {
            x: 16,
            y: 16,
            w: 32,
            h: 32,
        };
        assert!(equal_pixels(&a, &b, r));
        b.put_pixel(17, 17, Rgb([1, 0, 0]));
        assert!(!equal_pixels(&a, &b, r));
        assert!(equal_pixels(
            &a,
            &b,
            Region {
                x: 0,
                y: 0,
                w: 10,
                h: 10
            }
        ));
        assert!(!equal_pixels(&a, &RgbImage::new(65, 64), r));
    }
    #[test]
    fn bounded_age_and_resize() {
        let mut c = FrameCache {
            image: RgbImage::new(64, 64),
            regions: vec![],
            lines: vec![],
            refreshed: Instant::now(),
            frames: 0,
            edge: 960,
            temporal: None,
        };
        assert!(c.valid(&c.image, 960));
        assert!(!c.valid(&c.image, 1280));
        c.frames = 30;
        assert!(!c.valid(&c.image, 960));
        c.frames = 0;
        c.refreshed = Instant::now() - Duration::from_secs(11);
        assert!(!c.valid(&c.image, 960));
    }
}

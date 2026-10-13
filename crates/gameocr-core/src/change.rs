use image::{RgbImage, imageops::FilterType};

#[derive(Default)]
pub struct ChangeDetector {
    previous: Option<(u32, u32, Vec<u8>)>,
}
impl ChangeDetector {
    pub fn fingerprint(image: &RgbImage) -> Vec<u8> {
        image::imageops::resize(image, 64, 64, FilterType::Triangle).into_raw()
    }
    /// Compare against the last *successfully recognized* image, so small changes accumulate.
    pub fn changed(&self, image: &RgbImage, threshold: f64) -> bool {
        let Some((w, h, prev)) = &self.previous else {
            return true;
        };
        if (*w, *h) != image.dimensions() {
            return true;
        }
        let next = Self::fingerprint(image);
        let difference: u64 = prev
            .iter()
            .zip(&next)
            .map(|(a, b)| a.abs_diff(*b) as u64)
            .sum();
        difference > 0 && difference as f64 / (next.len() as f64 * 255.) >= threshold
    }
    pub fn commit(&mut self, image: &RgbImage) {
        self.previous = Some((image.width(), image.height(), Self::fingerprint(image)));
    }
    pub fn reset(&mut self) {
        self.previous = None;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn duplicate_threshold_and_resize() {
        let mut d = ChangeDetector::default();
        let a = RgbImage::new(100, 50);
        assert!(d.changed(&a, 0.03));
        d.commit(&a);
        assert!(!d.changed(&a, 0.));
        let b = RgbImage::from_pixel(100, 50, image::Rgb([4, 4, 4]));
        assert!(!d.changed(&b, 0.03));
        let c = RgbImage::from_pixel(100, 50, image::Rgb([40, 40, 40]));
        assert!(d.changed(&c, 0.03));
        assert!(d.changed(&RgbImage::new(101, 50), 0.03));
        d.reset();
        assert!(d.changed(&a, 0.03));
    }
}

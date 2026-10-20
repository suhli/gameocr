//! PP-OCR DB quad postprocessing, perspective crops and probability CTC decode.
//! Reference: PaddleX text_detection/processors.py and text_recognition/processors.py.
use anyhow::{Result, ensure};
use gameocr_core::{engine::OnnxConfig, types::Point};
use image::{GrayImage, Luma, Rgb, RgbImage};
use imageproc::{
    contours::find_contours,
    geometric_transformations::{Interpolation, Projection, warp_into},
    geometry::min_area_rect,
};

pub type Quad = [Point; 4];
pub fn distance(a: Point, b: Point) -> f64 {
    (a.x - b.x).hypot(a.y - b.y)
}
pub fn bbox(q: &Quad) -> [f64; 4] {
    q.iter().fold(
        [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ],
        |b, p| [b[0].min(p.x), b[1].min(p.y), b[2].max(p.x), b[3].max(p.y)],
    )
}
pub fn restore(q: Quad, map: (u32, u32), source: (u32, u32)) -> Quad {
    q.map(|p| Point {
        x: (p.x * source.0 as f64 / map.0 as f64)
            .round()
            .clamp(0., source.0.saturating_sub(1) as f64),
        y: (p.y * source.1 as f64 / map.1 as f64)
            .round()
            .clamp(0., source.1.saturating_sub(1) as f64),
    })
}
fn inside(x: f64, y: f64, q: &Quad) -> bool {
    let mut sign = 0.;
    for i in 0..4 {
        let a = q[i];
        let b = q[(i + 1) % 4];
        let cross = (b.x - a.x) * (y - a.y) - (b.y - a.y) * (x - a.x);
        if cross.abs() < 1e-6 {
            continue;
        }
        if sign == 0. {
            sign = cross.signum();
        } else if sign * cross < 0. {
            return false;
        }
    }
    true
}
/// For a minimum-area rectangle, the minimum rectangle around a round-join
/// polygon offset expands both axes by 2 * area * ratio / perimeter.
pub fn unclip(q: Quad, ratio: f32) -> Quad {
    let w = distance(q[0], q[1]);
    let h = distance(q[0], q[3]);
    if w < 1e-6 || h < 1e-6 {
        return q;
    }
    let d = w * h * ratio as f64 / (2. * (w + h));
    let u = Point {
        x: (q[1].x - q[0].x) / w,
        y: (q[1].y - q[0].y) / w,
    };
    let v = Point {
        x: (q[3].x - q[0].x) / h,
        y: (q[3].y - q[0].y) / h,
    };
    let signs = [(-1., -1.), (1., -1.), (1., 1.), (-1., 1.)];
    std::array::from_fn(|i| Point {
        x: q[i].x + d * (signs[i].0 * u.x + signs[i].1 * v.x),
        y: q[i].y + d * (signs[i].0 * u.y + signs[i].1 * v.y),
    })
}
pub fn detect_boxes(
    prob: &[f32],
    width: u32,
    height: u32,
    source: (u32, u32),
    config: &OnnxConfig,
) -> Result<Vec<Quad>> {
    ensure!(
        width > 0 && height > 0 && prob.len() == width as usize * height as usize,
        "Invalid detector output size"
    );
    ensure!(
        prob.iter()
            .all(|p| p.is_finite() && (0.0..=1.0).contains(p)),
        "Detector must output probabilities"
    );
    let bitmap = GrayImage::from_fn(width, height, |x, y| {
        Luma([
            if prob[(y * width + x) as usize] > config.detection_threshold {
                255
            } else {
                0
            },
        ])
    });
    let mut boxes = Vec::new();
    for contour in find_contours::<i32>(&bitmap).into_iter().take(3000) {
        if contour.points.len() < 3 {
            continue;
        }
        let q = min_area_rect(&contour.points).map(|p| Point {
            x: p.x as f64,
            y: p.y as f64,
        });
        if distance(q[0], q[1]).min(distance(q[0], q[3])) < 3. {
            continue;
        }
        let b = bbox(&q);
        let mut sum = 0.;
        let mut count = 0;
        for y in (b[1].floor().max(0.) as u32)..=(b[3].ceil().min((height - 1) as f64) as u32) {
            for x in (b[0].floor().max(0.) as u32)..=(b[2].ceil().min((width - 1) as f64) as u32) {
                if inside(x as f64, y as f64, &q) {
                    sum += prob[(y * width + x) as usize];
                    count += 1;
                }
            }
        }
        if count == 0 || sum / (count as f32) < config.box_threshold {
            continue;
        }
        let q = unclip(q, config.unclip_ratio);
        if distance(q[0], q[1]).min(distance(q[0], q[3])) < 5. {
            continue;
        }
        let q = restore(q, (width, height), source);
        if distance(q[0], q[1]).min(distance(q[0], q[3])) > 3. {
            boxes.push(q);
        }
    }
    reading_order(&mut boxes);
    Ok(boxes)
}
pub fn reading_order(boxes: &mut [Quad]) {
    // Paddle reading order: y first, then left to right within a 10px line.
    boxes.sort_by(|a, b| a[0].y.total_cmp(&b[0].y).then(a[0].x.total_cmp(&b[0].x)));
    for i in 1..boxes.len() {
        let mut j = i;
        while j > 0
            && (boxes[j][0].y - boxes[j - 1][0].y).abs() < 10.
            && boxes[j][0].x < boxes[j - 1][0].x
        {
            boxes.swap(j, j - 1);
            j -= 1;
        }
    }
}
fn crop_dimensions(q: &Quad) -> (u32, u32) {
    let w = distance(q[0], q[1])
        .max(distance(q[2], q[3]))
        .round()
        .max(1.) as u32;
    let h = distance(q[0], q[3])
        .max(distance(q[1], q[2]))
        .round()
        .max(1.) as u32;
    (w, h)
}
pub fn recognition_width(q: &Quad) -> usize {
    let (mut w, mut h) = crop_dimensions(q);
    if h as f64 / w as f64 >= 1.5 {
        std::mem::swap(&mut w, &mut h);
    }
    (48. * w as f64 / h as f64).ceil() as usize
}
pub fn crop(image: &RgbImage, q: &Quad) -> Result<RgbImage> {
    let (w, h) = crop_dimensions(q);
    ensure!(
        (w as u64) * (h as u64) <= 32_000_000,
        "Text crop exceeds memory budget"
    );
    let from = q.map(|p| (p.x as f32, p.y as f32));
    let to = [
        (0., 0.),
        (w as f32, 0.),
        (w as f32, h as f32),
        (0., h as f32),
    ];
    let projection = Projection::from_control_points(from, to)
        .ok_or_else(|| anyhow::anyhow!("Degenerate text quadrilateral"))?;
    let mut output = RgbImage::new(w, h);
    warp_into(
        image,
        &projection,
        Interpolation::Bilinear,
        Rgb([0, 0, 0]),
        &mut output,
    );
    Ok(if h as f64 / w as f64 >= 1.5 {
        image::imageops::rotate270(&output)
    } else {
        output
    })
}
pub fn decode_ctc(
    prob: &[f32],
    steps: usize,
    classes: usize,
    dictionary: &[String],
) -> Result<(String, f32)> {
    ensure!(
        classes == dictionary.len()
            && classes > 1
            && steps.checked_mul(classes) == Some(prob.len()),
        "Recognizer class count does not match dictionary"
    );
    let mut text = String::new();
    let mut confidence = 0.;
    let mut count = 0;
    let mut previous = 0;
    for row in prob.chunks_exact(classes) {
        let (mut index, mut score) = (0, f32::NEG_INFINITY);
        let mut valid = true;
        for (i, &value) in row.iter().enumerate() {
            valid &= value.is_finite() && (0.0..=1.00001).contains(&value);
            // Match Iterator::max_by: the last equal maximum wins.
            if value.total_cmp(&score).is_ge() {
                index = i;
                score = value;
            }
        }
        ensure!(valid, "Recognizer must output CTC probabilities");
        if index != 0 && index != previous {
            text.push_str(&dictionary[index]);
            confidence += score;
            count += 1;
        }
        previous = index;
    }
    Ok((
        text,
        if count == 0 {
            0.
        } else {
            confidence / count as f32
        },
    ))
}
pub fn read_dictionary(path: &std::path::Path) -> Result<Vec<String>> {
    let text = std::fs::read_to_string(path)?;
    let mut dictionary = vec![String::new()]; // CTC blank is always index zero.
    dictionary.extend(
        text.trim_start_matches('\u{feff}')
            .lines()
            .map(|s| s.trim_end_matches('\r').to_string()),
    );
    ensure!(
        dictionary.len() > 2 && dictionary.iter().skip(1).all(|s| !s.is_empty()),
        "Empty dictionary entry"
    );
    // Paddle's CTCLabelDecode appends space to character_dict.
    dictionary.push(" ".into());
    Ok(dictionary)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ctc_blank_repeats_and_multilingual() {
        let d = vec!["".into(), "中".into(), "A".into(), "あ".into(), "繁".into()];
        let ids = [1, 1, 0, 1, 2, 0, 3, 4];
        let mut p = vec![0.; ids.len() * 5];
        for (t, c) in ids.iter().enumerate() {
            p[t * 5 + c] = 0.95;
        }
        let (text, score) = decode_ctc(&p, ids.len(), 5, &d).unwrap();
        assert_eq!(text, "中中Aあ繁");
        assert!((score - 0.95).abs() < 1e-5);
        assert!(decode_ctc(&p, ids.len(), 4, &d).is_err());
        let d = vec!["".into(), "a".into(), "b".into()];
        assert_eq!(
            decode_ctc(&[0., 0.5, 0.5], 1, 3, &d).unwrap(),
            ("b".into(), 0.5)
        );
        for value in [f32::NAN, f32::INFINITY, -0.001, 1.1] {
            assert!(decode_ctc(&[value, 0.9, 0.1], 1, 3, &d).is_err());
        }
    }
    #[test]
    fn empty_detector_and_coordinate_restore() {
        let c = OnnxConfig::default();
        assert!(
            detect_boxes(&vec![0.; 1024], 32, 32, (640, 320), &c)
                .unwrap()
                .is_empty()
        );
        let mut p = vec![0.; 1024];
        for y in 10..20 {
            for x in 5..25 {
                p[y * 32 + x] = 0.9;
            }
        }
        let boxes = detect_boxes(&p, 32, 32, (640, 320), &c).unwrap();
        assert_eq!(boxes.len(), 1);
        let b = bbox(&boxes[0]);
        assert!(b[0] < 100. && b[2] > 480. && b[1] < 100.);
    }
    #[test]
    fn perspective_keeps_color_and_rotates_vertical() {
        let im = RgbImage::from_pixel(100, 100, Rgb([10, 20, 30]));
        let q = [
            Point { x: 20., y: 10. },
            Point { x: 40., y: 10. },
            Point { x: 40., y: 80. },
            Point { x: 20., y: 80. },
        ];
        let result = crop(&im, &q).unwrap();
        assert_eq!(result.dimensions(), (70, 20));
        assert_eq!(result.get_pixel(30, 10), &Rgb([10, 20, 30]));
    }
}

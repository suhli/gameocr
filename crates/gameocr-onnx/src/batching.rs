use crate::pipeline::{Quad, distance, recognition_width};
use gameocr_core::engine::OnnxConfig;

#[derive(Debug)]
pub struct Batch {
    pub indices: Vec<usize>,
    pub width: usize,
}
pub fn plan(boxes: &[Quad], config: &OnnxConfig) -> Vec<Batch> {
    let mut order: Vec<_> = (0..boxes.len()).collect();
    if !config.width_buckets {
        // Preserve the original sort and exact padding width by default.
        order.sort_by(|&a, &b| {
            let ratio = |i: usize| {
                let w = distance(boxes[i][0], boxes[i][1]);
                let h = distance(boxes[i][0], boxes[i][3]);
                w.max(h) / w.min(h).max(1.)
            };
            ratio(a).total_cmp(&ratio(b))
        });
        return order
            .chunks(config.batch_size)
            .map(|indices| Batch {
                indices: indices.to_vec(),
                width: indices
                    .iter()
                    .map(|&i| recognition_width(&boxes[i]))
                    .max()
                    .unwrap_or(320)
                    .clamp(320, 3200),
            })
            .collect();
    }
    let width = |i: usize| recognition_width(&boxes[i]).clamp(320, 3200).div_ceil(64) * 64;
    order.sort_by_key(|&i| width(i));
    let mut batches: Vec<Batch> = Vec::new();
    for index in order {
        let w = width(index);
        if let Some(last) = batches.last_mut() {
            if last.width == w
                && last.indices.len() < config.batch_size
                && (last.indices.len() + 1) * 48 * w <= config.recognition_pixel_budget
            {
                last.indices.push(index);
                continue;
            }
        }
        batches.push(Batch {
            indices: vec![index],
            width: w,
        });
    }
    batches
}
#[cfg(test)]
mod tests {
    use super::*;
    use gameocr_core::types::Point;
    fn q(w: f64) -> Quad {
        [
            Point { x: 0., y: 0. },
            Point { x: w, y: 0. },
            Point { x: w, y: 48. },
            Point { x: 0., y: 48. },
        ]
    }
    #[test]
    fn long_lines_do_not_inflate_short_batches_and_pixel_budget_holds() {
        let boxes = [q(200.), q(220.), q(250.), q(3000.), q(3100.), q(3200.)];
        let c = OnnxConfig {
            width_buckets: true,
            recognition_pixel_budget: 153600,
            ..Default::default()
        };
        let batches = plan(&boxes, &c);
        assert_eq!(batches[0].indices, vec![0, 1, 2]);
        assert_eq!(batches[0].width, 320);
        assert!(
            batches
                .iter()
                .all(|b| b.width * 48 * b.indices.len() <= 153600)
        );
        let mut indices = batches
            .iter()
            .flat_map(|b| b.indices.clone())
            .collect::<Vec<_>>();
        indices.sort();
        assert_eq!(indices, vec![0, 1, 2, 3, 4, 5]);
        let old = plan(&boxes, &OnnxConfig::default());
        assert_eq!(old.len(), 1);
        assert_eq!(old[0].width, 3200);
    }
}

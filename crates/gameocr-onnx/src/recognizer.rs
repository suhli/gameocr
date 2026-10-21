//! Explicit recognition-only entry shared by full and temporal pipelines.
use crate::ms;
use crate::{batching::Batch, pipeline};
use anyhow::{Result, ensure};
use gameocr_core::{engine::OnnxConfig, types::PipelineMetrics};
use image::RgbImage;
use ort::{session::Session, value::TensorRef};
use std::time::Instant;
pub struct Work {
    pub inference_ms: f64,
    pub preprocessing_ms: f64,
    pub parsing_ms: f64,
    pub batches: usize,
    pub pixels: usize,
}
#[allow(clippy::too_many_arguments)]
pub fn recognize_regions(
    recognizer: &mut Session,
    compact: &mut Option<Session>,
    dictionary: &[String],
    _config: &OnnxConfig,
    buffer: &mut Vec<f32>,
    image: &RgbImage,
    boxes: &[pipeline::Quad],
    batches: Vec<Batch>,
    decoded: &mut [Option<(String, f32)>],
    detail: &mut PipelineMetrics,
) -> Result<Work> {
    let mut recognition_ms = 0.;
    let mut preprocessing_ms = 0.;
    let mut parsing_ms = 0.;
    let mut recognition_batches = 0;
    let mut recognition_pixels = 0;
    for planned in batches {
        let batch: Vec<_> = planned
            .indices
            .into_iter()
            .filter(|&i| decoded[i].is_none())
            .collect();
        if batch.is_empty() {
            continue;
        }
        let width = planned.width;
        recognition_batches += 1;
        recognition_pixels += batch.len() * 48 * width;
        let prep = Instant::now();
        let plane = 48 * width;
        buffer.resize(batch.len() * 3 * plane, 0.);
        buffer.fill(0.); // Padding must not retain an earlier batch.
        for (n, &index) in batch.iter().enumerate() {
            let crop_start = Instant::now();
            let crop = pipeline::crop(&image, &boxes[index])?;
            detail.crop_ms += ms(crop_start);
            let rw = ((48. * crop.width() as f64 / crop.height() as f64).ceil() as usize)
                .min(width)
                .max(1);
            let resized = image::imageops::resize(
                &crop,
                rw as u32,
                48,
                image::imageops::FilterType::Triangle,
            );
            let sample = &mut buffer[n * 3 * plane..(n + 1) * 3 * plane];
            let (blue, rest) = sample.split_at_mut(plane);
            let (green, red) = rest.split_at_mut(plane);
            for (y, row) in resized.as_raw().chunks_exact(rw * 3).enumerate() {
                for (x, pixel) in row.chunks_exact(3).enumerate() {
                    let i = y * width + x;
                    blue[i] = pixel[2] as f32 / 127.5 - 1.;
                    green[i] = pixel[1] as f32 / 127.5 - 1.;
                    red[i] = pixel[0] as f32 / 127.5 - 1.;
                }
            }
        }
        let prep_ms = ms(prep);
        preprocessing_ms += prep_ms;
        detail.recognizer_preprocess_ms += prep_ms;
        detail.recognizer_calls += 1;
        detail.recognition_shapes.push([batch.len(), 3, 48, width]);
        if let Some(session) = compact.as_mut() {
            let start = Instant::now();
            let result = run_compact(session, buffer, batch.len(), width, dictionary);
            match result {
                Ok((values, bytes, inference, decode)) => {
                    recognition_ms += inference;
                    parsing_ms += decode;
                    detail.ctc_ms += decode;
                    detail.compact_batches += 1;
                    detail.output_bytes += bytes;
                    for (&index, value) in batch.iter().zip(values) {
                        decoded[index] = Some(value);
                    }
                    continue;
                }
                Err(e) => {
                    recognition_ms += ms(start);
                    detail.compact_fallbacks += 1;
                    detail.recognizer_calls += 1; // the fallback executes a second graph
                    detail.compact_status = format!("runtime fallback: {e}");
                    // Disable for this worker after any error, while retaining the original session.
                    *compact = None;
                }
            }
        }
        let rec_start = Instant::now();
        let output = recognizer.run(ort::inputs![TensorRef::from_array_view((
            [batch.len(), 3, 48, width],
            buffer.as_slice()
        ))?])?;
        recognition_ms += ms(rec_start);
        let post = Instant::now();
        let (shape, prob) = output[0].try_extract_tensor::<f32>()?;
        ensure!(
            shape.len() == 3
                && shape[0] == batch.len() as i64
                && shape[1] > 0
                && shape[2] == dictionary.len() as i64,
            "Invalid CTC output dimensions"
        );
        detail.output_bytes += prob.len() * 4;
        let steps = shape[1] as usize;
        let classes = shape[2] as usize;
        for (n, &index) in batch.iter().enumerate() {
            let (text, confidence) = pipeline::decode_ctc(
                &prob[n * steps * classes..(n + 1) * steps * classes],
                steps,
                classes,
                &dictionary,
            )?;
            decoded[index] = Some((text, confidence));
        }
        let post_ms = ms(post);
        parsing_ms += post_ms;
        detail.ctc_ms += post_ms;
    }
    Ok(Work {
        inference_ms: recognition_ms,
        preprocessing_ms,
        parsing_ms,
        batches: recognition_batches,
        pixels: recognition_pixels,
    })
}

pub fn compact_session(
    path: &std::path::Path,
    device: gameocr_core::types::Device,
    threads: u32,
) -> Result<Session> {
    let mut session = crate::session(path, device, threads, 3)?;
    let _ = run_compact(
        &mut session,
        &vec![0.; 3 * 48 * 320],
        1,
        320,
        &vec![String::new(); 18710],
    )?;
    crate::verify_cuda_execution(&mut session)?;
    Ok(session)
}
fn run_compact(
    session: &mut Session,
    buffer: &[f32],
    batch: usize,
    width: usize,
    dictionary: &[String],
) -> Result<(Vec<(String, f32)>, usize, f64, f64)> {
    let start = Instant::now();
    let output = session.run(ort::inputs![TensorRef::from_array_view((
        [batch, 3, 48, width],
        buffer
    ))?])?;
    let inference = ms(start);
    let decode = Instant::now();
    let (shape, ids) = output
        .get("gameocr_ctc_ids")
        .ok_or_else(|| anyhow::anyhow!("Missing compact CTC ids"))?
        .try_extract_tensor::<i64>()?;
    let (scores_shape, scores) = output
        .get("gameocr_ctc_scores")
        .ok_or_else(|| anyhow::anyhow!("Missing compact CTC scores"))?
        .try_extract_tensor::<f32>()?;
    let (valid_shape, valid) = output
        .get("gameocr_ctc_valid")
        .ok_or_else(|| anyhow::anyhow!("Missing compact CTC valid"))?
        .try_extract_tensor::<f32>()?;
    ensure!(
        shape.len() == 2
            && shape[0] == batch as i64
            && shape[1] > 0
            && shape == scores_shape
            && shape == valid_shape,
        "Invalid compact CTC dimensions"
    );
    let steps = shape[1] as usize;
    let mut values = Vec::new();
    for n in 0..batch {
        let r = n * steps..(n + 1) * steps;
        values.push(decode_compact(
            &ids[r.clone()],
            &scores[r.clone()],
            &valid[r],
            dictionary,
        )?);
    }
    Ok((values, ids.len() * 16, inference, ms(decode)))
}
fn decode_compact(
    ids: &[i64],
    scores: &[f32],
    valid: &[f32],
    dictionary: &[String],
) -> Result<(String, f32)> {
    ensure!(
        ids.len() == scores.len() && ids.len() == valid.len(),
        "Compact CTC length mismatch"
    );
    let mut text = String::new();
    let mut previous = 0;
    let mut sum = 0.;
    let mut count = 0;
    for ((&index, &score), &ok) in ids.iter().zip(scores).zip(valid) {
        // All-class validation is computed in the graph. Zero maxima fall back
        // to preserve total_cmp signed-zero tie semantics exactly.
        ensure!(
            ok == 1.
                && score.is_finite()
                && score > 0.
                && score <= 1.00001
                && index >= 0
                && (index as usize) < dictionary.len(),
            "Invalid compact CTC probability/class"
        );
        if index != 0 && index != previous {
            text.push_str(&dictionary[index as usize]);
            sum += score;
            count += 1;
        }
        previous = index;
    }
    Ok((text, if count == 0 { 0. } else { sum / count as f32 }))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compact_preserves_blank_repeat_confidence_and_rejects_invalid_nonmax() {
        let d = vec!["".into(), "中".into(), "A".into()];
        let (text, score) = decode_compact(&[1, 1, 0, 1, 2], &[0.9; 5], &[1.; 5], &d).unwrap();
        assert_eq!(text, "中中A");
        assert!((score - 0.9).abs() < 1e-6);
        for invalid in [0., f32::NAN, f32::INFINITY] {
            assert!(decode_compact(&[1], &[0.9], &[invalid], &d).is_err());
        }
        assert!(decode_compact(&[1], &[0.], &[1.], &d).is_err());
        assert!(decode_compact(&[3], &[0.9], &[1.], &d).is_err());
    }
}

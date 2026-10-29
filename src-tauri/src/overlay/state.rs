use anyhow::{Result, ensure};
use gameocr_core::types::{OcrBlock, OcrResult};
use gameocr_translation::{text::normalize, types::Identity};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

/// The fixed-position option belongs to a particular target, never to the
/// desktop globally. Following updates the anchor so turning it off freezes the
/// latest position rather than the position at application startup.
#[derive(Default)]
pub struct WindowAnchor(Option<(String, [i32; 4])>);
impl WindowAnchor {
    pub fn clear(&mut self) {
        self.0 = None;
    }
    pub fn allows(
        &mut self,
        target: &str,
        geometry: &crate::window::OverlayGeometry,
        follow: bool,
    ) -> bool {
        if !geometry.foreground || !geometry.visible {
            return false;
        }
        if follow || self.0.as_ref().is_none_or(|(id, _)| id != target) {
            self.0 = Some((target.into(), geometry.rect));
        }
        self.0
            .as_ref()
            .is_some_and(|(_, rect)| *rect == geometry.rect)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct OverlaySettings {
    pub overlay_enabled: bool,
    pub overlay_opacity: f64,
    pub overlay_blur: f64,
    pub overlay_font_size: f64,
    pub overlay_max_width_ratio: f64,
    pub overlay_padding: f64,
    pub overlay_corner_radius: f64,
    pub overlay_follow_window: bool,
    pub overlay_auto_hide_when_stale: bool,
    pub overlay_stale_ms: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::translation::TranslationEvent;
    use gameocr_translation::types::TranslationResult;
    #[test]
    fn window_anchor_handles_focus_visibility_target_changes_and_enable_cycles() {
        let mut anchor = WindowAnchor::default();
        let mut g = crate::window::OverlayGeometry {
            rect: [-800, 0, 0, 600],
            client_rect: [-800, 0, 0, 600],
            dpi: 144,
            mapping: "client".into(),
            foreground: true,
            visible: true,
        };
        assert!(anchor.allows("a", &g, false));
        g.foreground = false;
        assert!(!anchor.allows("a", &g, true));
        g.foreground = true;
        g.visible = false; // minimized/cloaked windows are not eligible
        assert!(!anchor.allows("a", &g, true));
        g.visible = true;
        g.rect = [50, 50, 850, 650];
        assert!(!anchor.allows("a", &g, false));
        // A target switch may occur entirely between two manager polls.
        assert!(anchor.allows("b", &g, false));
        g.rect = [100, 100, 900, 700];
        assert!(anchor.allows("b", &g, true));
        assert!(anchor.allows("b", &g, false));
        g.rect = [100, 100, 1000, 800];
        assert!(!anchor.allows("b", &g, false));
        for _ in 0..3 {
            anchor.clear(); // close/disable/invalid current identity
            assert!(anchor.allows("b", &g, false));
        }
    }
    fn block(id: &str, bbox: [f64; 4]) -> OcrBlock {
        OcrBlock {
            id: id.into(),
            text: "New Game".into(),
            label: None,
            bbox: Some(bbox),
            confidence: Some(1.),
            polygon: None,
        }
    }
    fn frame() -> OcrResult {
        OcrResult {
            frame_id: 1,
            timestamp: 1,
            source_width: 800,
            source_height: 600,
            blocks: vec![block("a", [20., 30., 150., 60.])],
            raw_output: String::new(),
            metrics: Default::default(),
            device: "CPU".into(),
            truncated: false,
            runtime: Some("onnx".into()),
            model_id: Some("ocr".into()),
            engine_id: None,
        }
    }
    fn identity() -> Identity {
        Identity {
            frame_id: 1,
            generation: 2,
            epoch: 3,
            revision: 4,
        }
    }
    fn event() -> TranslationEvent {
        TranslationEvent {
            identity: identity(),
            results: vec![TranslationResult {
                profile_id: String::new(),
                request_id: "r".into(),
                identity: identity(),
                source_model_id: "ocr".into(),
                translation_model_id: "hy-mt2".into(),
                source_language: "en".into(),
                target_language: "zh".into(),
                source_text: "New Game".into(),
                translated_text: "新游戏".into(),
                source_block_ids: vec!["a".into()],
                source_blocks: frame().blocks,
                glossary_version: 0,
                context_version: 0,
                cache_hit: false,
                status: "complete".into(),
                error: None,
                device: "CPU".into(),
                truncated: false,
                stop_reason: "eos".into(),
                metrics: Default::default(),
            }],
        }
    }
    #[test]
    fn settings_legacy_defaults_persistence_and_validation() {
        let defaults: OverlaySettings = serde_json::from_str("{}").unwrap();
        assert!(!defaults.overlay_enabled);
        assert!(defaults.overlay_follow_window && defaults.overlay_auto_hide_when_stale);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("overlay/settings.json");
        let mut s = defaults.clone();
        s.overlay_enabled = true;
        s.overlay_font_size = 20.;
        crate::settings::atomic_json(&path, &s).unwrap();
        assert_eq!(
            serde_json::from_slice::<OverlaySettings>(&std::fs::read(path).unwrap()).unwrap(),
            s
        );
        s.overlay_opacity = f64::NAN;
        assert!(s.validate().is_err());
        s = defaults;
        s.overlay_stale_ms = 30001;
        assert!(s.validate().is_err());
    }
    #[test]
    fn geometry_rejects_invalid_and_distant_groups_clips_and_uses_polygon() {
        assert_eq!(
            block_bounds(&block("a", [-10., 20., 90., 700.]), 800, 600),
            Some([0., 20., 90., 600.])
        );
        for bbox in [
            [0., 0., 0., 2.],
            [5., 4., 3., 8.],
            [f64::NAN, 0., 3., 8.],
            [900., 0., 1000., 10.],
        ] {
            assert!(block_bounds(&block("a", bbox), 800, 600).is_none());
        }
        let mut b = block("p", [0., 0., 0., 0.]);
        b.polygon = Some(vec![
            gameocr_core::types::Point { x: 5., y: 10. },
            gameocr_core::types::Point { x: 30., y: 10. },
            gameocr_core::types::Point { x: 20., y: 40. },
        ]);
        assert_eq!(block_bounds(&b, 800, 600), Some([5., 10., 30., 40.]));
        assert!(
            union_bounds(
                &[
                    block("a", [0., 0., 50., 20.]),
                    block("b", [700., 500., 750., 520.])
                ],
                800,
                600
            )
            .is_none()
        );
        assert_eq!(
            union_bounds(
                &[
                    block("a", [0., 0., 100., 20.]),
                    block("b", [0., 25., 100., 45.])
                ],
                800,
                600
            ),
            Some([0., 0., 100., 45.])
        );
    }
    #[test]
    fn same_text_relocates_without_flicker_and_old_frame_cannot_replace_it() {
        let mut live = LiveState::default();
        let mut ocr = frame();
        let mut id = identity();
        let now = Instant::now();
        live.observe(&ocr, &id, Some("window-a".into()), now);
        assert!(live.accept(&event()));
        for next in 2..12 {
            id.frame_id = next;
            ocr.frame_id = next;
            ocr.blocks[0].id = next.to_string();
            ocr.blocks[0].bbox = Some([220., 30., 350., 60.]);
            assert!(live.observe(&ocr, &id, Some("window-a".into()), now));
            assert_eq!(live.items.len(), 1);
            assert_eq!(live.items[0].bbox[0], 220.);
        }
        assert!(!live.accept(&event()));
        assert_eq!(live.items[0].text, "新游戏");
        id.revision += 1;
        ocr.blocks[0].text = "Load Game".into();
        assert!(!live.observe(&ocr, &id, Some("window-a".into()), now));
        assert!(live.items.is_empty());
        assert!(!live.accept(&event()));
    }
    #[test]
    fn lifecycle_invalidations_never_replay_history() {
        for change in 0..5 {
            let mut live = LiveState::default();
            let mut id = identity();
            let mut ocr = frame();
            live.observe(&ocr, &id, Some("a".into()), Instant::now());
            live.accept(&event());
            match change {
                0 => id.generation += 1,
                1 => id.epoch += 1,
                2 => id.revision += 1,
                3 => ocr.model_id = Some("new-ocr".into()),
                _ => {}
            }
            live.observe(
                &ocr,
                &id,
                Some(if change == 4 { "b" } else { "a" }.into()),
                Instant::now(),
            );
            assert!(live.items.is_empty());
            // Stop/pause/disable all use the same explicit state invalidation.
            live.clear();
            assert!(!live.accept(&event()));
            assert!(live.identity.is_none());
        }
    }
    #[test]
    fn only_complete_current_translations_are_visible() {
        for invalid in 0..9 {
            let mut live = LiveState::default();
            live.observe(&frame(), &identity(), Some("a".into()), Instant::now());
            let mut e = event();
            let r = &mut e.results[0];
            match invalid {
                0 => r.status = "incomplete".into(),
                1 => r.truncated = true,
                2 => r.error = Some("failed".into()),
                3 => r.stop_reason = "cancelled".into(),
                4 => r.translated_text = " ".into(),
                5 => r.source_language = "none".into(),
                6 => r.source_block_ids = vec!["unknown".into()],
                7 => r.source_model_id = "old".into(),
                _ => r.translated_text = r.source_text.clone(),
            }
            assert!(live.accept(&e));
            assert!(live.items.is_empty(), "invalid case {invalid}");
        }
    }
    #[test]
    fn expiry_is_bounded_even_when_early_auto_hide_is_disabled() {
        let now = Instant::now();
        let mut live = LiveState::default();
        let mut s = OverlaySettings::default();
        live.observe(&frame(), &identity(), Some("a".into()), now);
        assert!(live.fresh(&s, now + Duration::from_secs(14)));
        assert!(!live.fresh(&s, now + Duration::from_secs(16)));
        s.overlay_auto_hide_when_stale = false;
        assert!(live.fresh(&s, now + Duration::from_secs(29)));
        assert!(!live.fresh(&s, now + Duration::from_secs(31)));
        live.observe(
            &frame(),
            &identity(),
            Some("a".into()),
            now + Duration::from_secs(32),
        );
        assert!(live.fresh(&s, now + Duration::from_secs(33)));
        let mut empty = frame();
        empty.blocks.clear();
        live.observe(&empty, &identity(), Some("a".into()), now);
        assert!(live.items.is_empty());
    }
}
impl Default for OverlaySettings {
    fn default() -> Self {
        Self {
            overlay_enabled: false,
            overlay_opacity: 0.55,
            overlay_blur: 8.,
            overlay_font_size: 16.,
            overlay_max_width_ratio: 0.45,
            overlay_padding: 8.,
            overlay_corner_radius: 10.,
            overlay_follow_window: true,
            overlay_auto_hide_when_stale: true,
            overlay_stale_ms: 15000,
        }
    }
}
impl OverlaySettings {
    pub fn validate(&self) -> Result<()> {
        for (value, min, max) in [
            (self.overlay_opacity, 0.1, 0.9),
            (self.overlay_blur, 0., 16.),
            (self.overlay_font_size, 12., 36.),
            (self.overlay_max_width_ratio, 0.15, 0.8),
            (self.overlay_padding, 2., 20.),
            (self.overlay_corner_radius, 0., 24.),
        ] {
            ensure!(
                value.is_finite() && (min..=max).contains(&value),
                "字幕样式参数超出范围"
            );
        }
        ensure!(
            (2000..=30000).contains(&self.overlay_stale_ms),
            "字幕有效期必须为 2000–30000 ms"
        );
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Subtitle {
    pub id: String,
    pub text: String,
    pub bbox: [f64; 4],
    pub source_block_ids: Vec<String>,
    #[serde(skip)]
    pub indices: Vec<usize>,
}

/// Source coordinates are xyxy, never normalized or already DPI-scaled.
pub fn block_bounds(block: &OcrBlock, width: u32, height: u32) -> Option<[f64; 4]> {
    if width == 0 || height == 0 {
        return None;
    }
    let polygon = block.polygon.as_ref().filter(|p| {
        p.len() >= 3 && p.len() <= 64 && p.iter().all(|p| p.x.is_finite() && p.y.is_finite())
    });
    let bounds = polygon
        .map(|p| {
            p.iter().fold(
                [
                    f64::INFINITY,
                    f64::INFINITY,
                    f64::NEG_INFINITY,
                    f64::NEG_INFINITY,
                ],
                |mut b, p| {
                    b[0] = b[0].min(p.x);
                    b[1] = b[1].min(p.y);
                    b[2] = b[2].max(p.x);
                    b[3] = b[3].max(p.y);
                    b
                },
            )
        })
        .or(block.bbox)?;
    if !bounds.iter().all(|n| n.is_finite()) || bounds[2] <= bounds[0] || bounds[3] <= bounds[1] {
        return None;
    }
    let b = [
        bounds[0].max(0.),
        bounds[1].max(0.),
        bounds[2].min(width as f64),
        bounds[3].min(height as f64),
    ];
    (b[2] > b[0] && b[3] > b[1]).then_some(b)
}
pub fn union_bounds(blocks: &[OcrBlock], width: u32, height: u32) -> Option<[f64; 4]> {
    if blocks.is_empty() || blocks.len() > 32 {
        return None;
    }
    // Do not anchor a multi-block sentence to a partially missing location.
    let boxes: Vec<_> = blocks
        .iter()
        .map(|b| block_bounds(b, width, height))
        .collect::<Option<_>>()?;
    let mut bounds = boxes[0];
    let mut area = 0.;
    for b in &boxes {
        bounds = [
            bounds[0].min(b[0]),
            bounds[1].min(b[1]),
            bounds[2].max(b[2]),
            bounds[3].max(b[3]),
        ];
        area += (b[2] - b[0]) * (b[3] - b[1]);
    }
    let union_area = (bounds[2] - bounds[0]) * (bounds[3] - bounds[1]);
    // Widely separated UI items must not produce a screen-sized subtitle panel.
    if boxes.len() > 1
        && (union_area > area * 4. || union_area > width as f64 * height as f64 * 0.45)
    {
        return None;
    }
    Some(bounds)
}

#[derive(Default)]
pub struct LiveState {
    pub identity: Option<Identity>,
    pub target_id: Option<String>,
    pub source_width: u32,
    pub source_height: u32,
    pub model_id: Option<String>,
    pub blocks: Vec<OcrBlock>,
    pub items: Vec<Subtitle>,
    pub observed: Option<Instant>,
}
impl LiveState {
    pub fn clear(&mut self) {
        *self = Self::default();
    }
    pub fn observe(
        &mut self,
        ocr: &OcrResult,
        identity: &Identity,
        target: Option<String>,
        now: Instant,
    ) -> bool {
        let same = self.identity.as_ref().is_some_and(|old| {
            old.generation == identity.generation
                && old.epoch == identity.epoch
                && old.revision == identity.revision
        }) && self.target_id == target
            && self.model_id == ocr.model_id
            && self.blocks.len() == ocr.blocks.len()
            && self
                .blocks
                .iter()
                .zip(&ocr.blocks)
                .all(|(a, b)| normalize(&a.text) == normalize(&b.text));
        if !same {
            self.items.clear();
        }
        self.identity = Some(identity.clone());
        self.target_id = target;
        self.source_width = ocr.source_width;
        self.source_height = ocr.source_height;
        self.model_id = ocr.model_id.clone();
        self.blocks = ocr.blocks.clone();
        self.observed = Some(now);
        self.items.retain_mut(|item| {
            let Some(blocks) = item
                .indices
                .iter()
                .map(|&i| ocr.blocks.get(i).cloned())
                .collect::<Option<Vec<_>>>()
            else {
                return false;
            };
            let Some(bounds) = union_bounds(&blocks, ocr.source_width, ocr.source_height) else {
                return false;
            };
            item.bbox = bounds;
            item.source_block_ids = blocks.iter().map(|b| b.id.clone()).collect();
            true
        });
        same
    }
    pub fn accept(&mut self, event: &crate::translation::TranslationEvent) -> bool {
        if self.identity.as_ref() != Some(&event.identity) {
            return false;
        }
        self.items = event
            .results
            .iter()
            .filter_map(|r| {
                if r.identity != event.identity
                    || r.status != "complete"
                    || r.truncated
                    || r.error.is_some()
                    || r.stop_reason == "cancelled"
                    || r.stop_reason == "skip"
                    || r.source_language == "none"
                    || r.source_language == r.target_language
                    || r.translated_text.trim().is_empty()
                    || normalize(&r.translated_text) == normalize(&r.source_text)
                    || r.translated_text.chars().count() > 8192
                    || r.source_block_ids.is_empty()
                    || self.model_id.as_deref() != Some(&r.source_model_id)
                {
                    return None;
                }
                let indices = r
                    .source_block_ids
                    .iter()
                    .map(|id| self.blocks.iter().position(|b| &b.id == id))
                    .collect::<Option<Vec<_>>>()?;
                let blocks: Vec<_> = indices.iter().map(|&i| self.blocks[i].clone()).collect();
                // The current OCR owns geometry; never trust an older event's boxes.
                let bbox = union_bounds(&blocks, self.source_width, self.source_height)?;
                Some(Subtitle {
                    id: indices
                        .iter()
                        .map(usize::to_string)
                        .collect::<Vec<_>>()
                        .join(":"),
                    text: r.translated_text.trim().into(),
                    bbox,
                    source_block_ids: r.source_block_ids.clone(),
                    indices,
                })
            })
            .take(64)
            .collect();
        true
    }
    pub fn fresh(&self, settings: &OverlaySettings, now: Instant) -> bool {
        let ttl = if settings.overlay_auto_hide_when_stale {
            settings.overlay_stale_ms
        } else {
            30000
        };
        self.observed
            .is_some_and(|t| now.saturating_duration_since(t) <= Duration::from_millis(ttl))
    }
}

use super::{
    AdaptiveConfig, AdaptiveMetrics, StrategyId,
    features::{FallbackRiskEstimator, FrameFeatures},
    profiles::AdaptiveProfileStore,
};

#[derive(Default, Clone, Copy)]
pub struct ResourceState {
    pub cpu_load: Option<f64>,
    pub available_memory_mb: Option<u64>,
    pub previous_frame_ms: f64,
    pub minimum_memory_mb: u64,
}
impl ResourceState {
    pub fn constrained(self) -> bool {
        self.cpu_load.is_some_and(|v| v > 0.85)
            || self
                .available_memory_mb
                .is_some_and(|m| m < self.minimum_memory_mb.max(512))
            || self.previous_frame_ms > 1500.
    }
}
pub struct AdaptiveScheduler {
    opportunities: u64,
    last_exploration: u64,
    last_switch: u64,
    failures: u32,
}
impl Default for AdaptiveScheduler {
    fn default() -> Self {
        Self {
            opportunities: 0,
            last_exploration: 0,
            last_switch: 0,
            failures: 0,
        }
    }
}
impl AdaptiveScheduler {
    #[allow(clippy::too_many_arguments)]
    pub fn select(
        &mut self,
        store: &AdaptiveProfileStore,
        key: &str,
        f: &FrameFeatures,
        config: &AdaptiveConfig,
        current: StrategyId,
        can_parallel: bool,
        switch_ms: f64,
        resource: ResourceState,
        now: u64,
    ) -> AdaptiveMetrics {
        let matches = store.matching(key, f, now);
        let a = store.evidence(&matches, StrategyId::CpuSingle);
        let b = store.evidence(&matches, StrategyId::CpuParallel2);
        let risk = FallbackRiskEstimator::estimate(
            f,
            if b.successes >= 5 {
                Some(b.fallback_rate())
            } else {
                None
            },
        );
        let reliable = a.reliable() && b.reliable();
        let mut selected = StrategyId::CpuSingle;
        let mut reason = "历史证据不足，使用完整单 Worker 基线".to_string();
        if reliable {
            let preferred = matches
                .indices
                .first()
                .map(|&i| store.profiles[i].preferred)
                .unwrap_or(StrategyId::CpuSingle);
            if preferred == StrategyId::CpuParallel2
                && b.risks == 0
                && b.failure_rate() < 0.1
                && b.score().unwrap_or(f64::INFINITY) < a.score().unwrap_or(0.) * 0.9
            {
                selected = preferred;
                reason = "相似历史中并行稳定快于单实例，已通过样本数与风险门槛".into();
            } else {
                reason = "相似历史支持单 Worker，或并行收益不足 10%".into();
            }
        } else if risk > 0.65 {
            reason = "文字集中或接缝风险较高，保留完整单 Worker".into();
        }
        let mut exploring = false;
        // Deterministic epsilon schedule: no more than one in twenty eligible opportunities.
        // Fixed cadence is reproducible, with per-class and global wall-clock cooldowns.
        if config.learning
            && config.exploration
            && can_parallel
            && !resource.constrained()
            && self.failures < 2
            && f.valid()
            && f.edge_density > 0.001
            && risk < 0.7
        {
            self.opportunities += 1;
            let class_ready = matches
                .indices
                .iter()
                .all(|&i| now.saturating_sub(store.profiles[i].last_exploration_ms) >= 30_000);
            if self.opportunities.is_multiple_of(20)
                && now.saturating_sub(self.last_exploration) >= 30_000
                && class_ready
            {
                selected = selected.other();
                exploring = true;
                reason = "低频探索：在新的相似帧上试用另一策略；不视为严格同帧 A/B".into();
            }
        }
        if !can_parallel
            || resource
                .available_memory_mb
                .is_some_and(|m| m < resource.minimum_memory_mb.max(512))
        {
            selected = StrategyId::CpuSingle;
            exploring = false;
            reason = "资源上限或可用内存限制，使用单 Worker".into();
        }
        if selected != current && !exploring {
            let safe_return = selected == StrategyId::CpuSingle
                && (risk > 0.7 || self.failures > 0 || !can_parallel);
            let desired = if selected == StrategyId::CpuSingle {
                a.score()
            } else {
                b.score()
            };
            let actual = if current == StrategyId::CpuSingle {
                a.score()
            } else {
                b.score()
            };
            let amortized = desired
                .zip(actual)
                .is_some_and(|(d, c)| d + switch_ms / 20. >= c * 0.9);
            if !safe_return && (now.saturating_sub(self.last_switch) < 10_000 || amortized) {
                selected = current;
                reason = "切换冷却或预计收益不足以摊销模型重建成本，复用当前策略".into();
            }
        }
        let estimate = if selected == StrategyId::CpuSingle {
            &a
        } else {
            &b
        };
        AdaptiveMetrics {
            strategy: Some(selected),
            similar_samples: a.successes + b.successes,
            predicted_ms: estimate.score(),
            single_median_ms: a.quantile(0.5),
            parallel_median_ms: b.quantile(0.5),
            exploring,
            reliable,
            fallback_risk: risk,
            reason,
            match_kind: matches.kind.into(),
            cpu_load: resource.cpu_load,
            available_memory_mb: resource.available_memory_mb,
            ..Default::default()
        }
    }
    pub fn switched(&mut self, now: u64) {
        self.last_switch = now;
    }
    pub fn observed(&mut self, success: bool, exploring: bool, now: u64) {
        if success {
            self.failures = 0;
        } else {
            self.failures += 1;
        }
        if exploring {
            self.last_exploration = now;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adaptive::{features::FrameAnalyzer, profiles::Observation};
    fn f() -> FrameFeatures {
        FrameAnalyzer::default().analyze(&image::RgbImage::from_fn(1920, 1080, |x, y| {
            let on = (x % 900 < 180) && (y % 800 < 100) && (x % 17 < 8);
            image::Rgb([if on { 240 } else { 10 }; 3])
        }))
    }
    fn observation(ms: f64) -> Observation {
        Observation {
            latency_ms: ms,
            success: true,
            fallback: false,
            retries: 0,
            blocks: 8,
            quality_risk: false,
        }
    }
    #[test]
    fn learns_only_stable_comparable_evidence_and_isolates_fingerprints() {
        let f = f();
        let key = "a".repeat(64);
        let mut store = AdaptiveProfileStore::default();
        let mut scheduler = AdaptiveScheduler::default();
        let config = AdaptiveConfig {
            exploration: false,
            ..Default::default()
        };
        for _ in 0..4 {
            store.record(
                &key,
                f.clone(),
                StrategyId::CpuSingle,
                &observation(300.),
                100,
            );
            store.record(
                &key,
                f.clone(),
                StrategyId::CpuParallel2,
                &observation(180.),
                100,
            );
        }
        let pick = |s: &mut AdaptiveScheduler, st: &AdaptiveProfileStore, k: &str| {
            s.select(
                st,
                k,
                &f,
                &config,
                StrategyId::CpuSingle,
                true,
                200.,
                ResourceState::default(),
                50_000,
            )
        };
        assert_eq!(
            pick(&mut scheduler, &store, &key).strategy,
            Some(StrategyId::CpuSingle)
        );
        for _ in 0..8 {
            store.record(
                &key,
                f.clone(),
                StrategyId::CpuSingle,
                &observation(300.),
                100,
            );
            store.record(
                &key,
                f.clone(),
                StrategyId::CpuParallel2,
                &observation(180.),
                100,
            );
        }
        assert_eq!(
            pick(&mut scheduler, &store, &key).strategy,
            Some(StrategyId::CpuParallel2)
        );
        assert_eq!(
            pick(&mut scheduler, &store, &"b".repeat(64)).strategy,
            Some(StrategyId::CpuSingle)
        );
        for _ in 0..20 {
            store.record(
                &key,
                f.clone(),
                StrategyId::CpuParallel2,
                &observation(600.),
                60_000,
            );
        }
        assert_eq!(
            pick(&mut scheduler, &store, &key).strategy,
            Some(StrategyId::CpuSingle)
        );
        assert!(store.profiles[0].strategies[&StrategyId::CpuParallel2].recalibrations > 0);
        let restored = AdaptiveProfileStore::from_bytes(&serde_json::to_vec(&store).unwrap());
        assert_eq!(restored.profiles.len(), 1);
        assert!(
            AdaptiveProfileStore::from_bytes(b"broken")
                .summary
                .recovered
        );
    }
    #[test]
    fn exploration_rate_resource_and_cooldown_guards() {
        let mut f = f();
        f.components = 20;
        f.dispersion = 0.9;
        f.seam_text = 0.;
        f.seam_energy = 0.;
        f.crossing_regions = 0;
        f.imbalance = 0.1;
        let mut scheduler = AdaptiveScheduler::default();
        let store = AdaptiveProfileStore::default();
        let mut count = 0;
        for i in 1..=200 {
            let now = i * 31_000;
            let p = scheduler.select(
                &store,
                "key",
                &f,
                &AdaptiveConfig::default(),
                StrategyId::CpuSingle,
                true,
                200.,
                ResourceState::default(),
                now,
            );
            count += usize::from(p.exploring);
            scheduler.observed(true, p.exploring, now);
        }
        assert_eq!(count, 10);
        for i in 1..=100 {
            assert!(
                !scheduler
                    .select(
                        &store,
                        "key",
                        &f,
                        &AdaptiveConfig::default(),
                        StrategyId::CpuSingle,
                        true,
                        200.,
                        ResourceState {
                            cpu_load: Some(0.95),
                            ..Default::default()
                        },
                        i * 1000
                    )
                    .exploring
            );
        }
    }
    #[test]
    fn bounded_profiles_and_unknown_layouts() {
        let mut store = AdaptiveProfileStore::default();
        let features = f();
        for i in 0..300 {
            store.record(
                &format!("{i:064x}"),
                features.clone(),
                StrategyId::CpuSingle,
                &observation(100.),
                i,
            );
        }
        assert_eq!(store.profiles.len(), 256);
        assert!(
            serde_json::to_vec(&store).unwrap().len()
                < crate::adaptive::profiles::MAX_FILE_BYTES as usize
        );
        let blank = FrameAnalyzer::default().analyze(&image::RgbImage::new(1920, 1080));
        assert!(
            store
                .matching(&format!("{:064x}", 299), &blank, 400)
                .indices
                .is_empty()
        );
    }
}

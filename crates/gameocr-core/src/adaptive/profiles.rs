use super::{ALGORITHM_VERSION, FEATURE_VERSION, StrategyId, features::FrameFeatures};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const MAX_PROFILES: usize = 256;
pub const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;
const BINS: usize = 128;
const MONTH: u64 = 30 * 24 * 60 * 60 * 1000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StrategyStats {
    pub attempts: u64,
    pub successes: u64,
    pub failures: u64,
    pub fallbacks: u64,
    pub retries: u64,
    pub risks: u64,
    pub last_ms: u64,
    pub block_mean: f64,
    pub mean_ms: f64,
    pub deviation_ms: f64,
    pub bins: Vec<f64>,
    pub effective_samples: f64,
    pub drift_streak: u32,
    pub recalibrations: u64,
}
impl Default for StrategyStats {
    fn default() -> Self {
        Self {
            attempts: 0,
            successes: 0,
            failures: 0,
            fallbacks: 0,
            retries: 0,
            risks: 0,
            last_ms: 0,
            block_mean: 0.,
            mean_ms: 0.,
            deviation_ms: 0.,
            bins: vec![0.; BINS],
            effective_samples: 0.,
            drift_streak: 0,
            recalibrations: 0,
        }
    }
}
#[derive(Debug, Clone)]
pub struct Observation {
    pub latency_ms: f64,
    pub success: bool,
    pub fallback: bool,
    pub retries: u32,
    pub blocks: usize,
    pub quality_risk: bool,
}
impl StrategyStats {
    pub fn quantile(&self, q: f64) -> Option<f64> {
        let total: f64 = self.bins.iter().sum();
        if total <= 0. {
            return None;
        }
        let mut sum = 0.;
        for (i, v) in self.bins.iter().enumerate() {
            sum += v;
            if sum >= total * q {
                return Some(1.1f64.powf(i as f64 + 0.5));
            }
        }
        None
    }
    pub fn reliable(&self) -> bool {
        self.successes >= 5 && self.effective_samples >= 4.5
    }
    pub fn failure_rate(&self) -> f64 {
        self.failures as f64 / self.attempts.max(1) as f64
    }
    pub fn fallback_rate(&self) -> f64 {
        self.fallbacks as f64 / self.successes.max(1) as f64
    }
    pub fn score(&self) -> Option<f64> {
        Some(
            self.quantile(0.5)? * 0.8
                + self.quantile(0.95)? * 0.2
                + self.mean_ms
                    * (self.failure_rate() * 2.
                        + self.risks as f64 / self.attempts.max(1) as f64
                        + self.retries as f64 / self.attempts.max(1) as f64 * 0.2),
        )
    }
    pub fn observe(&mut self, o: &Observation, now: u64) {
        self.attempts += 1;
        self.last_ms = now;
        if !o.success {
            self.failures += 1;
            return;
        }
        if !o.latency_ms.is_finite() || o.latency_ms < 0. || o.latency_ms > 600_000. {
            self.failures += 1;
            return;
        }
        // Sustained drift expires old evidence; a single slow frame cannot reset a profile.
        if self.reliable()
            && (o.latency_ms > self.mean_ms * 1.5 || o.latency_ms < self.mean_ms * 0.6)
        {
            self.drift_streak += 1;
        } else {
            self.drift_streak = 0;
        }
        if self.drift_streak >= 3 {
            for n in &mut self.bins {
                *n *= 0.2;
            }
            self.effective_samples *= 0.2;
            self.drift_streak = 0;
            self.recalibrations += 1;
        }
        for n in &mut self.bins {
            *n *= 0.98;
        }
        let bin = (o.latency_ms.max(1.).ln() / 1.1f64.ln()).floor() as usize;
        self.bins[bin.min(BINS - 1)] += 1.;
        self.effective_samples = self.effective_samples * 0.98 + 1.;
        let alpha = if self.successes == 0 { 1. } else { 0.2 };
        let difference = if self.successes == 0 {
            0.
        } else {
            (o.latency_ms - self.mean_ms).abs()
        };
        self.mean_ms = self.mean_ms * (1. - alpha) + o.latency_ms * alpha;
        self.deviation_ms = self.deviation_ms * (1. - alpha) + difference * alpha;
        self.block_mean = self.block_mean * (1. - alpha) + o.blocks as f64 * alpha;
        self.successes += 1;
        self.fallbacks += u64::from(o.fallback);
        self.retries += o.retries as u64;
        self.risks += u64::from(o.quality_risk);
    }
    fn valid(&self) -> bool {
        self.bins.len() == BINS
            && self
                .bins
                .iter()
                .all(|v| v.is_finite() && (0.0..=1_000_000.0).contains(v))
            && self.attempts < 1_000_000_000
            && self.successes.saturating_add(self.failures) <= self.attempts
            && [
                self.block_mean,
                self.mean_ms,
                self.deviation_ms,
                self.effective_samples,
            ]
            .iter()
            .all(|v| v.is_finite() && *v >= 0.)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub id: u64,
    pub fingerprint: String,
    pub features: FrameFeatures,
    pub strategies: BTreeMap<StrategyId, StrategyStats>,
    pub preferred: StrategyId,
    pub updated_ms: u64,
    pub last_exploration_ms: u64,
    pub decisions: u64,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct LearningSummary {
    pub classes: usize,
    pub selections: BTreeMap<StrategyId, u64>,
    pub explorations: u64,
    pub successful_explorations: u64,
    pub switches: u64,
    pub fallbacks: u64,
    pub failed_frames: u64,
    pub recent_latency_ms: Option<f64>,
    pub recovered: bool,
    pub persistence_error: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdaptiveProfileStore {
    pub schema: u32,
    pub feature_version: u32,
    pub algorithm_version: u32,
    pub next_id: u64,
    pub profiles: Vec<Profile>,
    pub summary: LearningSummary,
}
impl Default for AdaptiveProfileStore {
    fn default() -> Self {
        Self {
            schema: 1,
            feature_version: FEATURE_VERSION,
            algorithm_version: ALGORITHM_VERSION,
            next_id: 1,
            profiles: Vec::new(),
            summary: Default::default(),
        }
    }
}
#[derive(Debug, Clone)]
pub struct ProfileMatch {
    pub indices: Vec<usize>,
    pub kind: &'static str,
}
impl AdaptiveProfileStore {
    pub fn from_bytes(bytes: &[u8]) -> Self {
        let parsed = if bytes.len() as u64 <= MAX_FILE_BYTES {
            serde_json::from_slice::<Self>(bytes).ok()
        } else {
            None
        };
        match parsed.filter(|s| {
            s.schema == 1
                && s.feature_version == FEATURE_VERSION
                && s.algorithm_version == ALGORITHM_VERSION
                && s.profiles.len() <= MAX_PROFILES
                && s.next_id < u64::MAX - 1024
                && s.profiles.iter().all(|p| {
                    p.fingerprint.len() == 64
                        && p.id < s.next_id
                        && p.decisions < 1_000_000_000
                        && p.features.valid()
                        && p.strategies.len() <= 2
                        && p.strategies.values().all(StrategyStats::valid)
                })
        }) {
            Some(mut s) => {
                s.summary.classes = s.profiles.len();
                s
            }
            None => {
                let mut s = Self::default();
                s.summary.recovered = true;
                s
            }
        }
    }
    pub fn matching(&self, key: &str, f: &FrameFeatures, now: u64) -> ProfileMatch {
        let compatible: Vec<_> = self
            .profiles
            .iter()
            .enumerate()
            .filter(|(_, p)| p.fingerprint == key && now.saturating_sub(p.updated_ms) < MONTH)
            .collect();
        let mut exact: Vec<_> = compatible
            .iter()
            .filter(|(_, p)| p.features.near_image(f))
            .map(|(i, p)| (*i, p.features.distance(f)))
            .collect();
        let kind = if exact.is_empty() {
            "workload"
        } else {
            "image"
        };
        if exact.is_empty() {
            exact = compatible
                .iter()
                .map(|(i, p)| (*i, p.features.distance(f)))
                .filter(|(_, d)| *d <= 0.075)
                .collect();
        }
        exact.sort_by(|a, b| a.1.total_cmp(&b.1));
        exact.truncate(3);
        ProfileMatch {
            kind: if exact.is_empty() { "unknown" } else { kind },
            indices: exact.into_iter().map(|(i, _)| i).collect(),
        }
    }
    pub fn evidence(&self, m: &ProfileMatch, id: StrategyId) -> StrategyStats {
        let mut out = StrategyStats::default();
        let mut weight = 0.;
        for &index in &m.indices {
            if let Some(s) = self.profiles[index].strategies.get(&id) {
                out.attempts += s.attempts;
                out.successes += s.successes;
                out.failures += s.failures;
                out.fallbacks += s.fallbacks;
                out.retries += s.retries;
                out.risks += s.risks;
                out.effective_samples += s.effective_samples;
                for (a, b) in out.bins.iter_mut().zip(&s.bins) {
                    *a += b;
                }
                out.mean_ms += s.mean_ms * s.effective_samples;
                out.block_mean += s.block_mean * s.effective_samples;
                out.deviation_ms += s.deviation_ms * s.effective_samples;
                weight += s.effective_samples;
            }
        }
        if weight > 0. {
            out.mean_ms /= weight;
            out.block_mean /= weight;
            out.deviation_ms /= weight;
        }
        out
    }
    pub fn record(
        &mut self,
        key: &str,
        f: FrameFeatures,
        id: StrategyId,
        o: &Observation,
        now: u64,
    ) -> u64 {
        self.profiles
            .retain(|p| now.saturating_sub(p.updated_ms) < MONTH * 3);
        // Store close exemplars, not an unbounded per-frame log. Cross-scene kNN is read-only.
        let existing = self
            .profiles
            .iter()
            .position(|p| p.fingerprint == key && p.features.near_image(&f));
        let index = existing.unwrap_or_else(|| {
            if self.profiles.len() >= MAX_PROFILES {
                let oldest = self
                    .profiles
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, p)| p.updated_ms)
                    .unwrap()
                    .0;
                self.profiles.remove(oldest);
            }
            let id = self.next_id;
            self.next_id = self.next_id.saturating_add(1);
            self.profiles.push(Profile {
                id,
                fingerprint: key.into(),
                features: f,
                strategies: BTreeMap::new(),
                preferred: StrategyId::CpuSingle,
                updated_ms: now,
                last_exploration_ms: 0,
                decisions: 0,
            });
            self.profiles.len() - 1
        });
        let p = &mut self.profiles[index];
        p.updated_ms = now;
        p.decisions += 1;
        p.strategies.entry(id).or_default().observe(o, now);
        // Promote only with enough samples on BOTH strategies and a stable, >10% risk-adjusted gain.
        if let (Some(a), Some(b)) = (
            p.strategies.get(&StrategyId::CpuSingle),
            p.strategies.get(&StrategyId::CpuParallel2),
        ) {
            if a.reliable() && b.reliable() {
                let sa = a.score().unwrap();
                let sb = b.score().unwrap();
                if sb < sa * 0.9
                    && b.risks == 0
                    && b.failure_rate() <= a.failure_rate()
                    && b.deviation_ms < b.mean_ms * 0.35
                {
                    p.preferred = StrategyId::CpuParallel2;
                } else if sa < sb * 0.9 || b.risks > 0 || b.failure_rate() > 0.1 {
                    p.preferred = StrategyId::CpuSingle;
                }
            }
        }
        *self.summary.selections.entry(id).or_default() += 1;
        self.summary.failed_frames += u64::from(!o.success);
        self.summary.fallbacks += u64::from(o.fallback);
        // The repository records end-to-end latency once, including switching.
        // Candidate histograms above intentionally contain steady execution cost.
        let profile_id = p.id;
        self.summary.classes = self.profiles.len();
        profile_id
    }
}

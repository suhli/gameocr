//! One coalescing writer, bounded snapshots, and generation-safe clearing.
use anyhow::{Result, bail};
use gameocr_core::adaptive::{
    StrategyId,
    features::FrameFeatures,
    profiles::{AdaptiveProfileStore, LearningSummary, MAX_FILE_BYTES, Observation},
};
use parking_lot::{Condvar, Mutex};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

pub struct LearningEvent {
    pub generation: u64,
    pub key: String,
    pub features: FrameFeatures,
    pub strategy: StrategyId,
    pub observation: Observation,
    pub now: u64,
    pub exploring: bool,
    pub switched: bool,
    pub total_ms: f64,
}
struct State {
    store: AdaptiveProfileStore,
    generation: u64,
    revision: u64,
    saved: u64,
    closed: bool,
}
struct Shared {
    state: Mutex<State>,
    wake: Condvar,
    path: PathBuf,
}
pub struct ProfileRepository {
    shared: Arc<Shared>,
    writer: Mutex<Option<std::thread::JoinHandle<()>>>,
}
impl ProfileRepository {
    pub fn open(path: &Path) -> Arc<Self> {
        let store = match std::fs::metadata(path) {
            Ok(m) if m.len() <= MAX_FILE_BYTES => {
                AdaptiveProfileStore::from_bytes(&std::fs::read(path).unwrap_or_default())
            }
            Ok(_) => AdaptiveProfileStore::from_bytes(b"oversized"),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => AdaptiveProfileStore::default(),
            Err(_) => AdaptiveProfileStore::from_bytes(b"unreadable"),
        };
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                store,
                generation: 0,
                revision: 0,
                saved: 0,
                closed: false,
            }),
            wake: Condvar::new(),
            path: path.into(),
        });
        let worker = shared.clone();
        let writer = std::thread::Builder::new()
            .name("adaptive-profile-writer".into())
            .spawn(move || {
                loop {
                    let (snapshot, revision) = {
                        let mut state = worker.state.lock();
                        while state.saved == state.revision && !state.closed {
                            worker.wake.wait(&mut state);
                        }
                        if state.closed && state.saved == state.revision {
                            break;
                        }
                        if !state.closed {
                            worker.wake.wait_for(&mut state, Duration::from_millis(250));
                        }
                        (state.store.clone(), state.revision)
                    };
                    let result = crate::settings::atomic_json(&worker.path, &snapshot);
                    let mut state = worker.state.lock();
                    state.saved = revision;
                    state.store.summary.persistence_error =
                        result.err().map(|e| format!("学习档案保存失败：{e:#}"));
                    worker.wake.notify_all();
                }
            })
            .expect("adaptive writer thread");
        Arc::new(Self {
            shared,
            writer: Mutex::new(Some(writer)),
        })
    }
    pub fn read<T>(&self, f: impl FnOnce(&AdaptiveProfileStore) -> T) -> T {
        f(&self.shared.state.lock().store)
    }
    pub fn generation(&self) -> u64 {
        self.shared.state.lock().generation
    }
    pub fn summary(&self) -> LearningSummary {
        self.read(|s| s.summary.clone())
    }
    pub fn commit(&self, event: LearningEvent) {
        let mut state = self.shared.state.lock();
        if state.generation != event.generation {
            return;
        }
        let id = state.store.record(
            &event.key,
            event.features,
            event.strategy,
            &event.observation,
            event.now,
        );
        if event.exploring {
            state.store.summary.explorations += 1;
            state.store.summary.successful_explorations +=
                u64::from(event.observation.success && !event.observation.quality_risk);
            if let Some(p) = state.store.profiles.iter_mut().find(|p| p.id == id) {
                p.last_exploration_ms = event.now;
            }
        }
        state.store.summary.switches += u64::from(event.switched);
        if event.observation.success {
            state.store.summary.recent_latency_ms = Some(
                state
                    .store
                    .summary
                    .recent_latency_ms
                    .map_or(event.total_ms, |v| v * 0.9 + event.total_ms * 0.1),
            );
        }
        state.revision += 1;
        self.shared.wake.notify_one();
    }
    pub fn clear(&self) {
        let mut state = self.shared.state.lock();
        state.store = AdaptiveProfileStore::default();
        state.generation += 1;
        state.revision += 1;
        self.shared.wake.notify_one();
    }
    pub fn flush(&self) -> Result<()> {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut state = self.shared.state.lock();
        while state.saved < state.revision {
            let now = Instant::now();
            if now >= deadline {
                bail!("等待学习档案写入超时");
            }
            self.shared.wake.notify_one();
            self.shared.wake.wait_for(&mut state, deadline - now);
        }
        if let Some(e) = &state.store.summary.persistence_error {
            bail!("{e}");
        }
        Ok(())
    }
}
impl Drop for ProfileRepository {
    fn drop(&mut self) {
        self.shared.state.lock().closed = true;
        self.shared.wake.notify_one();
        if let Some(writer) = self.writer.lock().take() {
            let _ = writer.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn persistence_clear_barrier_and_corrupt_recovery() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profiles.json");
        let repo = ProfileRepository::open(&path);
        let event = || LearningEvent {
            generation: 0,
            key: "a".repeat(64),
            features: gameocr_core::adaptive::features::FrameAnalyzer::default()
                .analyze(&image::RgbImage::new(100, 100)),
            strategy: StrategyId::CpuSingle,
            observation: Observation {
                latency_ms: 100.,
                success: true,
                fallback: false,
                retries: 0,
                blocks: 0,
                quality_risk: false,
            },
            now: 100,
            exploring: false,
            switched: false,
            total_ms: 200.,
        };
        repo.commit(event());
        let mut switched = event();
        switched.total_ms = 1200.;
        repo.commit(switched);
        assert_eq!(repo.summary().recent_latency_ms, Some(300.));
        repo.flush().unwrap();
        drop(repo);
        let repo = ProfileRepository::open(&path);
        assert_eq!(repo.summary().classes, 1);
        repo.clear();
        repo.commit(event());
        repo.flush().unwrap();
        drop(repo);
        assert_eq!(ProfileRepository::open(&path).summary().classes, 0);
        std::fs::write(&path, b"{bad json").unwrap();
        let repo = ProfileRepository::open(&path);
        assert!(repo.summary().recovered);
        repo.clear();
        repo.flush().unwrap();
        assert!(
            AdaptiveProfileStore::from_bytes(&std::fs::read(path).unwrap())
                .profiles
                .is_empty()
        );
    }
}

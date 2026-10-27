use crate::types::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::VecDeque;

pub fn key(
    manifest: &TranslationManifest,
    generation: &GenerationConfig,
    request: &TranslationRequest,
) -> String {
    // Identity is deliberately excluded: the caller attaches the current frame on a hit.
    let bytes = serde_json::to_vec(&(
        PROMPT_VERSION,
        &manifest.id,
        &manifest.revision,
        &manifest.sha256,
        &manifest.runtime,
        &manifest.adapter,
        generation,
        &request.source_text,
        &request.options,
    ))
    .expect("finite validated config");
    format!("{:x}", Sha256::digest(bytes))
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedText {
    pub key: String,
    pub text: String,
    pub truncated: bool,
}
#[derive(Default)]
pub struct TranslationCache {
    entries: VecDeque<CachedText>,
    capacity: usize,
    bytes: usize,
}
impl TranslationCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.min(4096),
            ..Self::default()
        }
    }
    pub fn get(&mut self, key: &str) -> Option<CachedText> {
        let index = self.entries.iter().position(|e| e.key == key)?;
        let entry = self.entries.remove(index)?;
        self.entries.push_front(entry.clone());
        Some(entry)
    }
    pub fn contains(&self, key: &str) -> bool {
        self.entries.iter().any(|e| e.key == key)
    }
    pub fn insert(&mut self, entry: CachedText) {
        // Also filters old persisted entries through restore(), preserving good entries.
        if entry.truncated || entry.text.trim().is_empty() {
            return;
        }
        if let Some(i) = self.entries.iter().position(|e| e.key == entry.key) {
            let old = self.entries.remove(i).unwrap();
            self.bytes -= old.text.len() + old.key.len();
        }
        self.bytes += entry.text.len() + entry.key.len();
        self.entries.push_front(entry);
        while self.entries.len() > self.capacity || self.bytes > 4 * 1024 * 1024 {
            if let Some(old) = self.entries.pop_back() {
                self.bytes -= old.text.len() + old.key.len();
            } else {
                break;
            }
        }
    }
    pub fn clear(&mut self) {
        self.entries.clear();
        self.bytes = 0;
    }
    pub fn export(&self) -> Vec<CachedText> {
        self.entries.iter().cloned().collect()
    }
    pub fn restore(&mut self, entries: Vec<CachedText>) {
        self.clear();
        for e in entries.into_iter().rev() {
            self.insert(e);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_results_and_legacy_entries_never_replace_complete_cache() {
        let entry = |key: &str, text: &str, truncated| CachedText {
            key: key.into(),
            text: text.into(),
            truncated,
        };
        let mut cache = TranslationCache::new(10);
        cache.restore(vec![
            entry("good", "完整", false),
            entry("old", "残", true),
            entry("empty", "  ", false),
        ]);
        assert!(cache.get("old").is_none());
        assert!(cache.get("empty").is_none());
        cache.insert(entry("good", "截", true));
        assert_eq!(cache.get("good").unwrap().text, "完整");
        assert_eq!(cache.export().len(), 1);
    }
    #[test]
    fn cache_keys_cover_model_parameters_profile_and_prompt_content() {
        let m = crate::catalog::hy_mt2();
        let g = GenerationConfig::default();
        let r = TranslationRequest {
            request_id: "id".into(),
            source_text: "Where are you going?".into(),
            options: Default::default(),
        };
        let base = key(&m, &g, &r);
        let mut revisions = Vec::new();
        let mut changed = m.clone();
        changed.revision = "other".into();
        revisions.push(key(&changed, &g, &r));
        let mut params = g.clone();
        params.temperature = 0.2;
        revisions.push(key(&m, &params, &r));
        for field in [
            "targetLanguage",
            "sourceLanguage",
            "style",
            "background",
            "profileId",
        ] {
            let mut value = serde_json::to_value(&r).unwrap();
            value["options"][field] = "changed".into();
            let changed: TranslationRequest = serde_json::from_value(value).unwrap();
            revisions.push(key(&m, &g, &changed));
        }
        let mut changed = r.clone();
        changed.options.context = vec!["Prior dialogue".into()];
        revisions.push(key(&m, &g, &changed));
        assert!(revisions.iter().all(|k| k != &base));
        let mut cache = TranslationCache::new(1);
        cache.restore(vec![
            CachedText {
                key: "a".into(),
                text: "one".into(),
                truncated: false,
            },
            CachedText {
                key: "b".into(),
                text: "two".into(),
                truncated: false,
            },
        ]);
        assert_eq!(cache.export().len(), 1);
        cache.clear();
        assert!(cache.get("a").is_none());
    }
    #[test]
    fn lru_and_prompt_isolation() {
        let m = crate::catalog::hy_mt2();
        let g = GenerationConfig::default();
        let mut r = TranslationRequest {
            request_id: "1".into(),
            source_text: "Phoenix".into(),
            options: PromptOptions::default(),
        };
        let first = key(&m, &g, &r);
        r.request_id = "2".into();
        assert_eq!(first, key(&m, &g, &r));
        r.options.glossary.push(Term {
            source: "Phoenix".into(),
            target: "菲尼克斯".into(),
            category: "人物".into(),
        });
        assert_ne!(first, key(&m, &g, &r));
        let mut c = TranslationCache::new(2);
        for k in ["a", "b"] {
            c.insert(CachedText {
                key: k.into(),
                text: k.into(),
                truncated: false,
            });
        }
        c.get("a");
        c.insert(CachedText {
            key: "c".into(),
            text: "c".into(),
            truncated: false,
        });
        assert!(c.get("b").is_none());
        assert!(c.get("a").is_some());
    }
}

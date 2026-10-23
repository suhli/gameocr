use crate::types::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::VecDeque;

pub fn key(manifest: &TranslationManifest, generation: &GenerationConfig, request: &TranslationRequest) -> String {
    // Identity is deliberately excluded: the caller attaches the current frame on a hit.
    let bytes = serde_json::to_vec(&(PROMPT_VERSION, &manifest.id, &manifest.revision, &manifest.sha256,
        &manifest.runtime, &manifest.adapter, generation, &request.source_text, &request.options)).expect("finite validated config");
    format!("{:x}", Sha256::digest(bytes))
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedText { pub key: String, pub text: String, pub truncated: bool }
#[derive(Default)]
pub struct TranslationCache { entries: VecDeque<CachedText>, capacity: usize, bytes: usize }
impl TranslationCache {
    pub fn new(capacity: usize) -> Self { Self { capacity:capacity.min(4096), ..Self::default() } }
    pub fn get(&mut self, key: &str) -> Option<CachedText> {
        let index = self.entries.iter().position(|e| e.key == key)?;
        let entry = self.entries.remove(index)?;
        self.entries.push_front(entry.clone()); Some(entry)
    }
    pub fn insert(&mut self, entry: CachedText) {
        if let Some(i) = self.entries.iter().position(|e| e.key == entry.key) {
            let old = self.entries.remove(i).unwrap(); self.bytes -= old.text.len() + old.key.len();
        }
        self.bytes += entry.text.len() + entry.key.len(); self.entries.push_front(entry);
        while self.entries.len() > self.capacity || self.bytes > 4 * 1024 * 1024 {
            if let Some(old) = self.entries.pop_back() { self.bytes -= old.text.len() + old.key.len(); } else { break }
        }
    }
    pub fn clear(&mut self) { self.entries.clear(); self.bytes = 0; }
    pub fn export(&self) -> Vec<CachedText> { self.entries.iter().cloned().collect() }
    pub fn restore(&mut self, entries: Vec<CachedText>) { self.clear(); for e in entries.into_iter().rev() { self.insert(e); } }
}
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn lru_and_prompt_isolation() {
        let m = crate::catalog::hy_mt2(); let g = GenerationConfig::default();
        let mut r = TranslationRequest { request_id:"1".into(), source_text:"Phoenix".into(), options:PromptOptions::default() };
        let first = key(&m,&g,&r); r.request_id = "2".into(); assert_eq!(first,key(&m,&g,&r));
        r.options.glossary.push(Term{source:"Phoenix".into(),target:"菲尼克斯".into(),category:"人物".into()}); assert_ne!(first,key(&m,&g,&r));
        let mut c = TranslationCache::new(2);
        for k in ["a","b"] { c.insert(CachedText{key:k.into(),text:k.into(),truncated:false}); }
        c.get("a"); c.insert(CachedText{key:"c".into(),text:"c".into(),truncated:false}); assert!(c.get("b").is_none()); assert!(c.get("a").is_some());
    }
}

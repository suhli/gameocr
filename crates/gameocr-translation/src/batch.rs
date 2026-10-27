//! Bounded structured batches. Never infer correspondence from output order.
use crate::cache::{self, TranslationCache};
use crate::types::*;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub struct Plan {
    pub ready: Vec<Option<GenerationResult>>,
    pub missing: Vec<usize>,
}
/// Snapshot hits before inserting batch output can evict them from a small LRU.
pub fn plan(
    requests: &[TranslationRequest],
    config: &TranslationLoad,
    cache: &mut TranslationCache,
) -> Plan {
    let mut missing = Vec::new();
    let ready = requests
        .iter()
        .enumerate()
        .map(|(index, r)| {
            let skip = r.options.source_language == "none"
                || r.options.source_language == r.options.target_language;
            let hit = if skip {
                None
            } else {
                cache.get(&cache::key(&config.manifest, &config.generation, r))
            };
            if !skip && hit.is_none() {
                missing.push(index);
                return None;
            }
            Some(GenerationResult {
                request_id: r.request_id.clone(),
                text: hit.map_or_else(|| r.source_text.clone(), |h| h.text),
                device: if skip { "skipped" } else { "cache" }.into(),
                truncated: false,
                stop_reason: if skip {
                    "sameLanguageOrNonText"
                } else {
                    "cache"
                }
                .into(),
                metrics: Default::default(),
            })
        })
        .collect();
    Plan { ready, missing }
}

pub fn validate_output(
    requests: &[TranslationRequest],
    output: &GenerationResult,
) -> Result<Vec<GenerationResult>> {
    ensure!(output.cacheable(), "翻译合批结果不完整");
    let texts = unpack(&output.text, requests.len())?;
    Ok(requests
        .iter()
        .zip(texts)
        .enumerate()
        .map(|(i, (request, text))| {
            let mut result = output.clone();
            result.request_id = request.request_id.clone();
            result.text = text;
            // One owner for additive inference/token totals; all items retain the batch ID.
            if i != 0 {
                result.metrics = Default::default();
            }
            result.metrics.batch_request_id = Some(output.request_id.clone());
            result.metrics.batch_size = requests.len();
            result
        })
        .collect())
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Item {
    id: String,
    text: String,
}

pub fn pack(requests: &[TranslationRequest], max_tokens: u32) -> Option<TranslationRequest> {
    if !(2..=8).contains(&requests.len())
        || max_tokens < requests.len() as u32 * 96
        || requests
            .iter()
            .any(|r| r.options != requests[0].options || r.source_text.len() > 512)
        || requests.iter().map(|r| r.source_text.len()).sum::<usize>() > 2048
    {
        return None;
    }
    let items: Vec<_> = requests
        .iter()
        .enumerate()
        .map(|(i, r)| Item {
            id: i.to_string(),
            text: r.source_text.clone(),
        })
        .collect();
    let mut options = requests[0].options.clone();
    options.batch_format = true;
    Some(TranslationRequest {
        request_id: format!("batch-{}", requests[0].request_id),
        source_text: serde_json::to_string(&items).ok()?,
        options,
    })
}

pub fn unpack(text: &str, count: usize) -> Result<Vec<String>> {
    // Do not repair malformed JSON, match lines by position, or accept partial output.
    let items: Vec<Item> = serde_json::from_str(text)?;
    ensure!(items.len() == count, "翻译合批条目数量不匹配");
    let mut by_id = HashMap::new();
    for item in items {
        ensure!(!item.text.trim().is_empty(), "翻译合批返回空文本");
        ensure!(
            by_id.insert(item.id, item.text).is_none(),
            "翻译合批编号重复"
        );
    }
    (0..count)
        .map(|i| {
            by_id
                .remove(&i.to_string())
                .ok_or_else(|| anyhow::anyhow!("翻译合批编号缺失或未知"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (Vec<TranslationRequest>, TranslationLoad) {
        let config = TranslationLoad {
            manifest: crate::catalog::hy_mt2(),
            path: String::new(),
            device: gameocr_core::types::Device::Cpu,
            threads: 1,
            generation: GenerationConfig {
                max_tokens: 1024,
                ..Default::default()
            },
        };
        (
            (0..6)
                .map(|i| TranslationRequest {
                    request_id: format!("request-{i}"),
                    source_text: format!("Menu item {i}"),
                    options: PromptOptions {
                        source_language: "en".into(),
                        ..Default::default()
                    },
                })
                .collect(),
            config,
        )
    }
    #[test]
    fn only_misses_are_batched_and_hits_survive_lru_eviction() {
        let (requests, config) = fixture();
        for cached in [0, 3, 5, 6] {
            let mut cache = TranslationCache::new(6);
            for request in &requests[..cached] {
                cache.insert(crate::cache::CachedText {
                    key: cache::key(&config.manifest, &config.generation, request),
                    text: "cached".into(),
                    truncated: false,
                });
            }
            let planned = plan(&requests, &config, &mut cache);
            assert_eq!(
                planned
                    .ready
                    .iter()
                    .flatten()
                    .filter(|r| r.device == "cache")
                    .count(),
                cached
            );
            assert_eq!(planned.missing, (cached..6).collect::<Vec<_>>());
            let misses: Vec<_> = planned
                .missing
                .iter()
                .map(|&i| requests[i].clone())
                .collect();
            let packed = pack(&misses, config.generation.max_tokens);
            assert_eq!(packed.is_some(), 6 - cached >= 2);
            if let Some(packed) = packed {
                let items: Vec<Item> = serde_json::from_str(&packed.source_text).unwrap();
                assert_eq!(items.len(), 6 - cached);
                assert_eq!(items[0].text, requests[cached].source_text);
            }
            cache.clear();
            assert_eq!(planned.ready.iter().flatten().count(), cached);
        }
    }
    #[test]
    fn skips_budgets_and_partial_failure_keep_cache_safe_and_totals_unique() {
        let (mut requests, config) = fixture();
        requests[0].options.source_language = "none".into();
        requests[1].options.source_language = "zh".into();
        let mut cache = TranslationCache::new(10);
        let planned = plan(&requests, &config, &mut cache);
        assert_eq!(planned.missing, vec![2, 3, 4, 5]);
        assert!(
            planned.ready[..2]
                .iter()
                .flatten()
                .all(|r| r.device == "skipped")
        );
        let misses = &requests[2..];
        assert!(pack(misses, 383).is_none());
        assert!(pack(misses, 384).is_some());
        let mut output = GenerationResult { request_id: "batch".into(), text: r#"[{"id":"3","text":"D"},{"id":"1","text":"B"},{"id":"0","text":"A"},{"id":"2","text":"C"}]"#.into(), device: "cpu".into(), truncated: false, stop_reason: "eos".into(), metrics: TranslationMetrics { generated_tokens: 80, inference_ms: 100., ..Default::default() } };
        let results = validate_output(misses, &output).unwrap();
        assert_eq!(
            results.iter().map(|r| r.text.as_str()).collect::<Vec<_>>(),
            vec!["A", "B", "C", "D"]
        );
        assert_eq!(
            results
                .iter()
                .map(|r| r.metrics.generated_tokens)
                .sum::<usize>(),
            80
        );
        assert_eq!(
            results.iter().map(|r| r.metrics.inference_ms).sum::<f64>(),
            100.
        );
        for (r, original) in results.iter().zip(misses) {
            assert_eq!(r.request_id, original.request_id);
        }
        output.truncated = true;
        assert!(validate_output(misses, &output).is_err());
        output.truncated = false;
        output.text = r#"[{"id":"0","text":"partial"}]"#.into();
        assert!(validate_output(misses, &output).is_err());
        assert!(cache.export().is_empty());
        requests[2].source_text = "x".repeat(513);
        assert!(pack(&requests[2..], 1024).is_none());
    }
    #[test]
    fn reordered_ids_are_safe_but_missing_duplicate_and_malformed_output_is_rejected() {
        assert_eq!(
            unpack(
                r#"[{"id":"1","text":"载入"},{"id":"0","text":"新游戏"}]"#,
                2
            )
            .unwrap(),
            ["新游戏", "载入"]
        );
        for invalid in [
            r#"[{"id":"0","text":"新游戏"}]"#,
            r#"[{"id":"0","text":"新游戏"},{"id":"0","text":"载入"}]"#,
            r#"[{"id":"0","text":"新游戏"},{"id":"9","text":"载入"}]"#,
            r#"[{"id":"0","text":""},{"id":"1","text":"载入"}]"#,
            "新游戏\n载入",
        ] {
            assert!(unpack(invalid, 2).is_err());
        }
    }
}

//! Bounded structured batches. Never infer correspondence from output order.
use crate::types::*;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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

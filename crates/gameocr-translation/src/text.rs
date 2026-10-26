use crate::types::TextGroup;
use gameocr_core::types::OcrBlock;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Grouping {
    Block,
    Paragraph,
    #[default]
    Auto,
}
pub fn normalize(s: &str) -> String {
    s.replace("\r\n", "\n")
        .replace('\r', "\n")
        .lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}
pub fn language(s: &str, hint: &str) -> String {
    let kana = s.chars().any(|c| ('\u{3040}'..='\u{30ff}').contains(&c));
    let han = s.chars().any(|c| ('\u{3400}'..='\u{9fff}').contains(&c));
    let latin = s.chars().filter(|c| c.is_ascii_alphabetic()).count();
    if !s.chars().any(char::is_alphabetic) {
        return "none".into();
    }
    if kana {
        return if latin >= 8 { "mixed" } else { "ja" }.into();
    }
    // Han alone is ambiguous (Japanese names and Chinese share the script).
    if han {
        return if hint != "auto" && !hint.is_empty() {
            hint
        } else if latin > 3 {
            "mixed"
        } else {
            "und"
        }
        .into();
    }
    if hint != "auto" && !hint.is_empty() {
        return hint.into();
    }
    let words = s
        .split_whitespace()
        .filter(|w| w.chars().any(|c| c.is_ascii_alphabetic()))
        .count();
    let english_evidence = s.split(|c: char| !c.is_ascii_alphabetic()).any(|word| {
        [
            "the",
            "you",
            "your",
            "where",
            "what",
            "this",
            "that",
            "please",
            "press",
            "health",
            "quest",
            "settings",
            "inventory",
            "game",
        ]
        .contains(&word.to_ascii_lowercase().as_str())
    });
    if latin >= 6 && words >= 2 && english_evidence {
        "en"
    } else {
        "und"
    }
    .into()
}
pub fn groups(blocks: &[OcrBlock], mode: Grouping) -> Vec<TextGroup> {
    let mut ordered: Vec<_> = blocks
        .iter()
        .filter(|b| !normalize(&b.text).is_empty())
        .cloned()
        .collect();
    ordered.sort_by(|a, b| match (a.bbox, b.bbox) {
        (Some(a), Some(b)) => a[1].total_cmp(&b[1]).then_with(|| a[0].total_cmp(&b[0])),
        _ => std::cmp::Ordering::Equal,
    });
    let mut out: Vec<TextGroup> = Vec::new();
    for b in ordered {
        let text = normalize(&b.text);
        let merge = out.last().is_some_and(|last| match mode {
            Grouping::Block => false,
            Grouping::Paragraph => last.source_text.len() + text.len() < 8192,
            Grouping::Auto => {
                let prev = last.blocks.last().unwrap();
                match (prev.bbox, b.bbox) {
                    // Bboxes are [left, top, right, bottom]. Conservative continuation
                    // requires a sentence fragment, compatible font height and aligned lines.
                    (Some(a), Some(c)) => {
                        let h = (a[3] - a[1]).max(1.0);
                        let gap = c[1] - a[3];
                        let fragment = prev.text.chars().count() >= 18
                            && !prev
                                .text
                                .trim_end()
                                .ends_with(['.', '!', '?', '。', '！', '？', ':', '：']);
                        fragment
                            && gap >= -h * 0.2
                            && gap <= h * 0.65
                            && (a[0] - c[0]).abs() <= h * 1.5
                            && ((c[3] - c[1]) / h - 1.0).abs() < 0.4
                            && last.source_text.len() + text.len() < 4096
                    }
                    _ => false,
                }
            }
        });
        if merge {
            let last = out.last_mut().unwrap();
            last.source_text.push('\n');
            last.source_text.push_str(&text);
            last.blocks.push(b);
        } else {
            out.push(TextGroup {
                source_text: text,
                blocks: vec![b],
            });
        }
    }
    out
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn output_budget_keeps_complete_unicode_but_rejects_invalid_bytes() {
        assert_eq!(decode_output(vec![b'A', 0xe4, 0xbd], true).unwrap(), "A");
        assert!(decode_output(vec![0xe4, 0xbd], false).is_err());
        assert!(decode_output(vec![0xff, b'A'], true).is_err());
    }
    #[test]
    fn languages_do_not_guess_han_or_names() {
        assert_eq!(language("東京", "auto"), "und");
        assert_eq!(language("東京", "ja"), "ja");
        assert_eq!(language("東京へ行こう", "auto"), "ja");
        assert_eq!(language("HP: 100/200", "auto"), "und");
        assert_eq!(language("100 / 200", "auto"), "none");
        assert_eq!(language("Where are you going?", "auto"), "en");
        assert_eq!(language("Bonjour mon ami", "auto"), "und");
        assert_eq!(language("Quest Clear! お疲れさま", "auto"), "mixed");
    }
    fn block(text: &str, y: f64) -> OcrBlock {
        OcrBlock {
            id: format!("{y}"),
            text: text.into(),
            label: None,
            bbox: Some([0., y, 300., y + 20.]),
            confidence: None,
            polygon: None,
        }
    }
    #[test]
    fn grouping_preserves_blocks_and_separates_menu() {
        let b = vec![block("Where are you going", 0.), block("this time?", 24.)];
        let g = groups(&b, Grouping::Auto);
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].blocks, b);
        assert_eq!(
            groups(
                &[block("Save game", 0.), block("Load game", 24.)],
                Grouping::Auto
            )
            .len(),
            2
        );
    }
}
/// Byte-level tokenizers can end a token budget in the middle of one character.
/// Keep complete characters and let the result's `truncated` flag explain the cut.
pub fn decode_output(mut bytes: Vec<u8>, truncated: bool) -> anyhow::Result<String> {
    if truncated {
        if let Err(error) = std::str::from_utf8(&bytes) {
            if error.error_len().is_none() {
                bytes.truncate(error.valid_up_to());
            }
        }
    }
    Ok(String::from_utf8(bytes)?)
}

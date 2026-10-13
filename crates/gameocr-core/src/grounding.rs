//! LightOnOCR-3's official `![label](x1,y1,x2,y2)` format.
//! Invalid/incomplete markers are retained as text, never invented coordinates.
use crate::types::OcrBlock;
use regex::Regex;
use std::sync::LazyLock;

static MARKER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"!\[([a-zA-Z_]+\+?)\]\(([^)\r\n]*)\)").unwrap());

pub fn pixels(coords: &str, width: u32, height: u32) -> Option<[f64; 4]> {
    let values: Vec<f64> = coords
        .split(',')
        .map(str::trim)
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    if values.len() != 4 || values.iter().any(|x| !x.is_finite()) || width == 0 || height == 0 {
        return None;
    }
    let [x1, y1, x2, y2] = [values[0], values[1], values[2], values[3]];
    if x2 <= x1 || y2 <= y1 {
        return None;
    }
    let b = [
        x1.clamp(0., 1000.) * width as f64 / 1000.,
        y1.clamp(0., 1000.) * height as f64 / 1000.,
        x2.clamp(0., 1000.) * width as f64 / 1000.,
        y2.clamp(0., 1000.) * height as f64 / 1000.,
    ];
    (b[2] > b[0] && b[3] > b[1]).then_some(b)
}

pub fn parse(raw: &str, width: u32, height: u32) -> Vec<OcrBlock> {
    let matches: Vec<_> = MARKER.captures_iter(raw).collect();
    let mut out = Vec::new();
    let mut push = |text: &str, label: Option<String>, bbox: Option<[f64; 4]>| {
        let text = text.trim();
        if !text.is_empty() || bbox.is_some() {
            out.push(OcrBlock {
                id: format!("block-{}", out.len()),
                text: text.to_owned(),
                label,
                bbox,
            });
        }
    };
    let first = matches
        .first()
        .map_or(raw.len(), |c| c.get(0).unwrap().start());
    push(&raw[..first], None, None);
    for (i, cap) in matches.iter().enumerate() {
        let start = cap.get(0).unwrap().end();
        let end = matches
            .get(i + 1)
            .map_or(raw.len(), |c| c.get(0).unwrap().start());
        push(
            &raw[start..end],
            Some(cap[1].to_owned()),
            pixels(&cap[2], width, height),
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn official_multilingual_multiline() {
        let b = parse(
            "![title](80,70,920,140)\n# Game 游戏\n![text+](0,200,1000,400)\nこんにちは\nHP 100",
            1920,
            1080,
        );
        assert_eq!(b.len(), 2);
        assert_eq!(b[0].bbox, Some([153.6, 75.6, 1766.4, 151.2]));
        assert_eq!(b[1].text, "こんにちは\nHP 100");
        assert_eq!(b[1].label.as_deref(), Some("text+"));
    }
    #[test]
    fn plain_and_missing_coordinates_survive() {
        let b = parse("intro\n![text](bad)\nhello\n![text](1,2", 100, 100);
        assert_eq!(b.len(), 2);
        assert!(b[1].bbox.is_none());
        assert!(b[1].text.contains("![text](1,2"));
    }
    #[test]
    fn clamp_and_reject() {
        assert_eq!(
            pixels("-10,0,1100,1000", 200, 100),
            Some([0., 0., 200., 100.])
        );
        for s in [
            "4,0,3,9",
            "NaN,0,4,5",
            "0,0,inf,5",
            "1,2,3",
            "0,2,4,2",
            "-20,0,-10,10",
        ] {
            assert!(pixels(s, 100, 100).is_none(), "{s}");
        }
    }
    #[test]
    fn empty_and_image_regions() {
        assert!(parse(" \n ", 1, 1).is_empty());
        assert_eq!(parse("![image](0,0,1000,1000)", 10, 10).len(), 1);
    }
}

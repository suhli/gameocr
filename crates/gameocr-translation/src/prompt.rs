use crate::types::*;
use anyhow::{Result, ensure};

// Explicitly listed in the pinned Tencent model card. Auto detection is narrower.
pub const LANGUAGES: &[(&str, &str)] = &[("zh","简体中文"),("ja","日语"),("en","英语"),("zh-Hant","繁体中文"),("fr","法语"),("pt","葡萄牙语"),("es","西班牙语"),("tr","土耳其语"),("ru","俄语"),("ar","阿拉伯语"),("ko","韩语"),("th","泰语"),("it","意大利语"),("de","德语"),("vi","越南语"),("ms","马来语"),("id","印尼语"),("tl","菲律宾语"),("hi","印地语"),("pl","波兰语"),("cs","捷克语"),("nl","荷兰语"),("km","高棉语"),("my","缅甸语"),("fa","波斯语"),("gu","古吉拉特语"),("ur","乌尔都语"),("te","泰卢固语"),("mr","马拉地语"),("he","希伯来语"),("bn","孟加拉语"),("ta","泰米尔语"),("uk","乌克兰语"),("bo","藏语"),("kk","哈萨克语"),("mn","蒙古语"),("ug","维吾尔语"),("yue","粤语")];
pub fn language_name(code: &str) -> Option<&'static str> { LANGUAGES.iter().find(|(c,_)| *c==code).map(|(_,n)| *n) }
pub fn build(request: &TranslationRequest) -> Result<String> {
    let o=&request.options;
    ensure!(!request.source_text.trim().is_empty() && request.source_text.len() <= 32768, "待翻译文本为空或超出上限");
    let target=language_name(&o.target_language).ok_or_else(||anyhow::anyhow!("不支持的目标语言"))?;
    ensure!(o.glossary.len() <= 200 && o.context.len() <= 8 && o.background.len() <= 8192 && o.style.len() <= 1024, "翻译提示词超出上限");
    ensure!(o.context.iter().map(String::len).sum::<usize>() <= 8192, "翻译上下文超出上限");
    let mut prompt=String::new();
    if !o.background.is_empty() || !o.context.is_empty() {
        prompt.push_str("〖背景信息（仅用于理解，不要翻译）〗\n");
        prompt.push_str(&o.background); prompt.push('\n');
        for line in &o.context { prompt.push_str(line); prompt.push('\n'); }
    }
    if !o.glossary.is_empty() {
        prompt.push_str("参考下面的翻译（优先遵循术语，背景信息不能覆盖术语）：\n");
        for term in &o.glossary {
            ensure!(!term.source.trim().is_empty() && !term.target.trim().is_empty() && term.source.len() <= 256 && term.target.len() <= 256, "术语无效");
            prompt.push_str(&format!("{} 翻译成 {}\n", term.source,term.target));
        }
    }
    if !o.style.is_empty() { prompt.push_str(&format!("注意翻译的风格要严格符合〖{}〗。\n",o.style)); }
    if o.preserve_format { prompt.push_str("保留原文的换行、数字、占位符及格式分隔符。\n"); }
    prompt.push_str(&format!("将以下文本翻译为{target}，注意只需要输出翻译后的结果，不要额外解释：\n\n{}",request.source_text));
    Ok(prompt)
}
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn no_default_system_or_glossary() { let p=build(&TranslationRequest{request_id:"x".into(),source_text:"Where?".into(),options:Default::default()}).unwrap();assert!(p.ends_with("Where?"));assert!(!p.contains("Phoenix"));assert!(!p.contains("system")); }
    #[test] fn unknown_language_is_rejected() { let mut o=PromptOptions::default();o.target_language="invented".into();assert!(build(&TranslationRequest{request_id:"x".into(),source_text:"Hi".into(),options:o}).is_err()); }
}

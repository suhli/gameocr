use anyhow::{Result, ensure};
use gameocr_core::types::Device;
use gameocr_translation::{text::Grouping, types::*};
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct TranslationSettings {
    pub enabled: bool,
    pub device: Device,
    pub resource_policy: String,
    pub gpu_reserve_mb: u32,
    pub stability: String,
    pub debounce_ms: u64,
    pub grouping: Grouping,
    pub source_language: String,
    pub target_language: String,
    pub style: String,
    pub threads: u32,
    pub generation: GenerationConfig,
    pub cache_capacity: usize,
    pub persistent_cache: bool,
    pub context_enabled: bool,
    pub context_lines: usize,
    pub profile_id: Option<String>,
    pub profiles: Vec<GameProfile>,
}
impl Default for TranslationSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            device: Device::Auto,
            resource_policy: "balanced".into(),
            gpu_reserve_mb: 512,
            stability: "balanced".into(),
            debounce_ms: 350,
            grouping: Grouping::Auto,
            source_language: "auto".into(),
            target_language: "zh".into(),
            style: String::new(),
            threads: 4,
            generation: Default::default(),
            cache_capacity: 512,
            persistent_cache: false,
            context_enabled: false,
            context_lines: 3,
            profile_id: None,
            profiles: Vec::new(),
        }
    }
}
impl TranslationSettings {
    pub fn validate(&self) -> Result<()> {
        self.generation.validate()?;
        ensure!(
            (128..=8192).contains(&self.gpu_reserve_mb),
            "翻译显存预留必须为 128–8192 MiB"
        );
        ensure!(
            (1..=64).contains(&self.threads)
                && self.debounce_ms <= 3000
                && self.cache_capacity <= 4096
                && self.context_lines <= 8,
            "翻译预算超出范围"
        );
        ensure!(
            ["balanced", "fast", "cpuOnly", "manual"].contains(&self.resource_policy.as_str()),
            "翻译资源策略无效"
        );
        ensure!(
            ["fast", "balanced", "accurate"].contains(&self.stability.as_str()),
            "翻译稳定模式无效"
        );
        ensure!(
            self.source_language == "auto"
                || gameocr_translation::prompt::language_name(&self.source_language).is_some(),
            "翻译源语言无效"
        );
        ensure!(
            gameocr_translation::prompt::language_name(&self.target_language).is_some(),
            "翻译目标语言无效"
        );
        ensure!(
            self.style.len() <= 1024 && self.profiles.len() <= 100,
            "翻译配置超出容量"
        );
        let mut ids = std::collections::HashSet::new();
        for p in &self.profiles {
            p.validate()?;
            ensure!(ids.insert(&p.id), "Profile ID 重复");
        }
        ensure!(
            self.profile_id.as_ref().is_none_or(|id| ids.contains(id)),
            "选中的 Profile 不存在"
        );
        Ok(())
    }
    pub fn options(&self, context: Vec<String>, context_version: u64) -> PromptOptions {
        let p = self
            .profile_id
            .as_ref()
            .and_then(|id| self.profiles.iter().find(|p| &p.id == id));
        PromptOptions {
            profile_id: self.profile_id.clone().unwrap_or_default(),
            source_language: self.source_language.clone(),
            target_language: self.target_language.clone(),
            style: if self.style.is_empty() {
                p.map_or(String::new(), |p| p.style.clone())
            } else {
                self.style.clone()
            },
            glossary: p.map_or(Vec::new(), |p| p.glossary.clone()),
            background: if self.context_enabled {
                p.map_or(String::new(), |p| p.background.clone())
            } else {
                String::new()
            },
            glossary_version: p.map_or(0, |p| p.version),
            context_version: if self.context_enabled {
                context_version
            } else {
                0
            },
            context: if self.context_enabled {
                context
            } else {
                Vec::new()
            },
            preserve_format: true,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn migration_disabled_and_context_profile_isolation() {
        let mut s: TranslationSettings = serde_json::from_str("{}").unwrap();
        assert!(!s.enabled);
        assert!(!s.persistent_cache);
        assert!(s.validate().is_ok());
        let a = s.options(vec!["secret".into()], 42);
        assert!(a.context.is_empty());
        assert_eq!(a.context_version, 0);
        s.context_enabled = true;
        s.profile_id = Some("game-a".into());
        s.profiles.push(GameProfile {
            id: "game-a".into(),
            name: "A".into(),
            version: 2,
            ..Default::default()
        });
        assert!(s.validate().is_ok());
        let b = s.options(vec!["confirmed dialogue".into()], 42);
        assert_eq!(b.profile_id, "game-a");
        assert_eq!(b.context_version, 42);
        s.profiles.push(s.profiles[0].clone());
        assert!(s.validate().is_err());
    }
}

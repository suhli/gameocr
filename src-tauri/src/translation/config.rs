use anyhow::{Context, Result, ensure};
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
    pub batch_translation: bool,
    pub source_language: String,
    pub target_language: String,
    // None inherits the selected Profile, or the global language without a Profile.
    pub source_language_override: Option<String>,
    pub target_language_override: Option<String>,
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
            batch_translation: true,
            source_language: "auto".into(),
            target_language: "zh".into(),
            source_language_override: None,
            target_language_override: None,
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
    pub fn from_saved(bytes: &[u8]) -> Result<Self> {
        let value: serde_json::Value = serde_json::from_slice(bytes)?;
        let mut s: Self = serde_json::from_value(value.clone())?;
        // Old UI copied Profile languages into global fields. Preserve a later manual
        // edit, but let copied defaults follow subsequent Profile edits.
        if let Some(p) = s.selected_profile() {
            let source = s.source_language.clone();
            let target = s.target_language.clone();
            let source_changed =
                source != p.source_language && !(p.source_language.is_empty() && source == "auto");
            let target_changed =
                target != p.target_language && !(p.target_language.is_empty() && target == "zh");
            if value.get("sourceLanguageOverride").is_none() && source_changed {
                s.source_language_override = Some(source);
            }
            if value.get("targetLanguageOverride").is_none() && target_changed {
                s.target_language_override = Some(target);
            }
        }
        Ok(s)
    }
    fn selected_profile(&self) -> Option<&GameProfile> {
        self.profile_id
            .as_ref()
            .and_then(|id| self.profiles.iter().find(|p| &p.id == id))
    }
    pub fn update_profile_versions(&mut self, previous: &Self) -> Result<()> {
        for profile in &mut self.profiles {
            if let Some(old) = previous.profiles.iter().find(|p| p.id == profile.id) {
                let changed = profile.source_language != old.source_language
                    || profile.target_language != old.target_language
                    || profile.style != old.style
                    || profile.background != old.background
                    || profile.glossary != old.glossary;
                let minimum = if changed {
                    old.version.checked_add(1).context("Profile 版本已达上限")?
                } else {
                    old.version
                };
                profile.version = profile.version.max(minimum);
            } else {
                profile.version = profile.version.max(1);
            }
        }
        Ok(())
    }
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
            self.source_language_override.as_ref().is_none_or(
                |l| l == "auto" || gameocr_translation::prompt::language_name(l).is_some()
            ),
            "翻译源语言覆盖无效"
        );
        ensure!(
            self.target_language_override
                .as_ref()
                .is_none_or(|l| gameocr_translation::prompt::language_name(l).is_some()),
            "翻译目标语言覆盖无效"
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
        let p = self.selected_profile();
        PromptOptions {
            batch_format: false,
            profile_id: self.profile_id.clone().unwrap_or_default(),
            source_language: self
                .source_language_override
                .as_ref()
                .or_else(|| p.map(|p| &p.source_language).filter(|l| !l.is_empty()))
                .unwrap_or(&self.source_language)
                .clone(),
            target_language: self
                .target_language_override
                .as_ref()
                .or_else(|| {
                    p.map(|p| &p.target_language)
                        .filter(|l| !l.is_empty() && *l != "auto")
                })
                .unwrap_or(&self.target_language)
                .clone(),
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
    fn overrides_migrate_and_effective_languages_change_prompt_and_cache() {
        let bytes = br#"{"profileId":"a","profiles":[{"id":"a","sourceLanguage":"ja","targetLanguage":"en"}],"sourceLanguage":"ja","targetLanguage":"zh"}"#;
        let mut s = TranslationSettings::from_saved(bytes).unwrap();
        assert_eq!(s.source_language_override, None);
        assert_eq!(s.target_language_override.as_deref(), Some("zh"));
        s.profiles[0].source_language = "en".into();
        assert_eq!(s.options(vec![], 0).source_language, "en");
        s.source_language_override = Some("ja".into());
        let s = TranslationSettings::from_saved(&serde_json::to_vec(&s).unwrap()).unwrap();
        let r = TranslationRequest {
            request_id: "id".into(),
            source_text: "東京".into(),
            options: s.options(vec![], 0),
        };
        assert_eq!(r.options.source_language, "ja");
        assert!(
            gameocr_translation::prompt::build(&r)
                .unwrap()
                .contains("源语言为日语")
        );
        let key = gameocr_translation::cache::key(
            &gameocr_translation::catalog::hy_mt2(),
            &s.generation,
            &r,
        );
        let mut changed = r.clone();
        changed.options.target_language = "en".into();
        assert_ne!(
            key,
            gameocr_translation::cache::key(
                &gameocr_translation::catalog::hy_mt2(),
                &s.generation,
                &changed
            )
        );
    }
    #[test]
    fn profile_versions_only_track_semantics_and_never_decrease() {
        let mut old = TranslationSettings::default();
        old.profiles = vec![
            GameProfile {
                id: "a".into(),
                version: 7,
                ..Default::default()
            },
            GameProfile {
                id: "b".into(),
                version: 3,
                ..Default::default()
            },
        ];
        let mut next = old.clone();
        next.device = Device::Cuda;
        next.debounce_ms = 99;
        next.update_profile_versions(&old).unwrap();
        assert_eq!(next.profiles, old.profiles);
        for field in [
            "glossary",
            "background",
            "style",
            "sourceLanguage",
            "targetLanguage",
        ] {
            let mut value = serde_json::to_value(&old).unwrap();
            value["profiles"][0][field] = if field == "glossary" {
                serde_json::json!([{"source":"a", "target":"b"}])
            } else {
                serde_json::json!("en")
            };
            let mut next: TranslationSettings = serde_json::from_value(value).unwrap();
            next.update_profile_versions(&old).unwrap();
            assert_eq!(next.profiles[0].version, 8);
            assert_eq!(next.profiles[1].version, 3);
            let saved = next.clone();
            next.update_profile_versions(&saved).unwrap();
            assert_eq!(next.profiles, saved.profiles);
        }
        next.profiles[0].version = 99;
        next.profiles[0].style = "new".into();
        next.update_profile_versions(&old).unwrap();
        assert_eq!(next.profiles[0].version, 99);
        let saved = next.clone();
        next.update_profile_versions(&saved).unwrap();
        assert_eq!(next.profiles[0].version, 99);
        next.profiles[0].version = 1;
        next.update_profile_versions(&saved).unwrap();
        assert_eq!(next.profiles[0].version, 99);
    }
    #[test]
    fn selected_profile_languages_survive_edit_save_and_reload() {
        let mut s = TranslationSettings::default();
        assert_eq!(s.options(vec![], 0).source_language, "auto");
        assert_eq!(s.options(vec![], 0).target_language, "zh");
        s.profiles = vec![
            GameProfile {
                id: "a".into(),
                source_language: "ja".into(),
                target_language: "en".into(),
                ..Default::default()
            },
            GameProfile {
                id: "b".into(),
                source_language: "en".into(),
                target_language: "ja".into(),
                ..Default::default()
            },
        ];
        for (id, source, target) in [("a", "ja", "en"), ("b", "en", "ja")] {
            s.profile_id = Some(id.into());
            let options = s.options(vec![], 0);
            assert_eq!(options.source_language, source);
            assert_eq!(options.target_language, target);
        }
        s.profiles[1].target_language = "zh".into();
        let loaded: TranslationSettings =
            serde_json::from_slice(&serde_json::to_vec(&s).unwrap()).unwrap();
        assert_eq!(loaded.options(vec![], 0).target_language, "zh");
    }
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

use crate::types::*;
use anyhow::{Result, ensure};

pub trait TranslationEngine {
    fn translate(&mut self, request: TranslationRequest) -> Result<GenerationResult>;
    fn device(&self) -> &str;
}
pub struct TranslationFactory;
impl TranslationFactory {
    pub fn validate(config: &TranslationLoad) -> Result<()> {
        config.manifest.validate()?; config.generation.validate()?;
        ensure!((1..=64).contains(&config.threads), "翻译线程数无效");
        ensure!(config.manifest.runtime == RUNTIME && config.manifest.adapter == ADAPTER, "此翻译 Runtime/Adapter 尚未注册");
        ensure!(config.device != gameocr_core::types::Device::Auto, "设备回退由 TranslationManager 管理，Worker 只接受明确设备");
        Ok(())
    }
    #[cfg(feature="inference")]
    pub fn create(config: TranslationLoad) -> Result<Box<dyn TranslationEngine>> {
        Self::validate(&config)?;
        Ok(Box::new(crate::runtime::HyMtAdapter::load(config)?))
    }
}

use crate::types::*;
use anyhow::{Result, ensure};
use std::sync::{Arc, atomic::AtomicBool};

pub trait TranslationEngine {
    fn translate(&mut self, request: TranslationRequest) -> Result<GenerationResult> {
        self.translate_cancellable(request, Arc::new(AtomicBool::new(false)))
    }
    fn translate_cancellable(
        &mut self,
        request: TranslationRequest,
        cancel: Arc<AtomicBool>,
    ) -> Result<GenerationResult>;
    fn device(&self) -> &str;
}
pub struct TranslationFactory;
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn factory_checks_runtime_not_file_extension() {
        let mut config = TranslationLoad {
            manifest: crate::catalog::hy_mt2(),
            path: "model.gguf".into(),
            device: gameocr_core::types::Device::Cpu,
            threads: 4,
            generation: Default::default(),
        };
        assert!(TranslationFactory::validate(&config).is_ok());
        config.manifest.adapter = "lightonocr".into();
        assert!(TranslationFactory::validate(&config).is_err());
        config.manifest = crate::catalog::hy_mt2();
        config.manifest.file = "../outside.gguf".into();
        assert!(TranslationFactory::validate(&config).is_err());
        config.manifest = crate::catalog::hy_mt2();
        config.generation.max_tokens = config.generation.context_size;
        assert!(TranslationFactory::validate(&config).is_err());
    }
}
impl TranslationFactory {
    pub fn validate(config: &TranslationLoad) -> Result<()> {
        config.manifest.validate()?;
        config.generation.validate()?;
        ensure!((1..=64).contains(&config.threads), "翻译线程数无效");
        ensure!(
            config.manifest.runtime == RUNTIME && config.manifest.adapter == ADAPTER,
            "此翻译 Runtime/Adapter 尚未注册"
        );
        ensure!(
            config.device != gameocr_core::types::Device::Auto,
            "设备回退由 TranslationManager 管理，Worker 只接受明确设备"
        );
        Ok(())
    }
    #[cfg(feature = "inference")]
    pub fn create(config: TranslationLoad) -> Result<Box<dyn TranslationEngine>> {
        Self::validate(&config)?;
        Ok(Box::new(crate::runtime::HyMtAdapter::load(config)?))
    }
}

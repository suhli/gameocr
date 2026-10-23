use crate::{prompt::LANGUAGES, types::*};
pub fn hy_mt2() -> TranslationManifest {
    TranslationManifest {
        schema_version:1, id:"hy-mt2-1.8b-q4-k-m".into(), name:"Hy-MT2 1.8B Q4_K_M".into(),
        runtime:RUNTIME.into(), adapter:ADAPTER.into(), revision:"a0c709d9fac510f2c807aa3af52872340dc37a4a".into(),
        quantization:"Q4_K_M".into(), license:"Apache-2.0".into(), source:"https://huggingface.co/tencent/Hy-MT2-1.8B-GGUF".into(),
        file:"Hy-MT2-1.8B-Q4_K_M.gguf".into(), size:1_133_080_448,
        sha256:"dc5f44fcf1fa496ee7ad725982c0c8c553a4de00259b53af84c4b89fb0c06699".into(),
        url:Some("https://huggingface.co/tencent/Hy-MT2-1.8B-GGUF/resolve/a0c709d9fac510f2c807aa3af52872340dc37a4a/Hy-MT2-1.8B-Q4_K_M.gguf".into()),
        languages:LANGUAGES.iter().map(|(code,_)|code.to_string()).collect(), imported:false, installed:false,
    }
}

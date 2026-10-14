use crate::engine::OcrCapabilities;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelFile {
    pub role: String,
    pub path: String,
    pub size: u64,
    pub sha256: String,
    #[serde(default)]
    pub url: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelManifest {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub runtime: String,
    pub adapter: String,
    pub version: String,
    pub quantization: String,
    pub capabilities: OcrCapabilities,
    pub files: Vec<ModelFile>,
    pub source: String,
    #[serde(default)]
    pub parameters: serde_json::Value,
    #[serde(default)]
    pub installed: bool,
    #[serde(default)]
    pub imported: bool,
}
impl ModelManifest {
    pub fn file(&self, role: &str) -> Result<&ModelFile> {
        self.files
            .iter()
            .find(|f| f.role == role)
            .with_context(|| format!("Missing model file role: {role}"))
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.schema_version == 2,
            "Unsupported manifest version {}",
            self.schema_version
        );
        ensure!(safe_id(&self.id), "Unsafe model ID");
        ensure!(!self.files.is_empty(), "Empty model manifest");
        let mut roles = std::collections::HashSet::new();
        for file in &self.files {
            ensure!(safe_file(&file.path), "Unsafe model file path");
            ensure!(roles.insert(&file.role), "Duplicate model file role");
            ensure!(
                file.size > 0
                    && file.sha256.len() == 64
                    && file.sha256.bytes().all(|b| b.is_ascii_hexdigit()),
                "File size/hash missing or invalid"
            );
        }
        Ok(())
    }
    /// Version 1 files remain in place. Migration is metadata-only and lossless.
    pub fn from_slice(bytes: &[u8]) -> Result<Self> {
        let v: serde_json::Value = serde_json::from_slice(bytes)?;
        let result = if v.get("schemaVersion").is_some() {
            serde_json::from_value(v)?
        } else {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct Legacy {
                id: String,
                name: String,
                quantization: String,
                model_file: String,
                mmproj_file: String,
                model_size: u64,
                mmproj_size: u64,
                model_sha256: String,
                mmproj_sha256: String,
                installed: bool,
                imported: bool,
            }
            let m: Legacy = serde_json::from_value(v)?;
            Self {
                schema_version: 2,
                id: m.id,
                name: m.name,
                runtime: "llamacpp".into(),
                adapter: "lightonocr".into(),
                version: "3".into(),
                quantization: m.quantization,
                capabilities: OcrCapabilities::lighton(),
                files: vec![
                    ModelFile {
                        role: "main".into(),
                        path: m.model_file,
                        size: m.model_size,
                        sha256: m.model_sha256,
                        url: None,
                    },
                    ModelFile {
                        role: "mmproj".into(),
                        path: m.mmproj_file,
                        size: m.mmproj_size,
                        sha256: m.mmproj_sha256,
                        url: None,
                    },
                ],
                source: "legacy".into(),
                parameters: serde_json::Value::Null,
                installed: m.installed,
                imported: m.imported,
            }
        };
        result.validate()?;
        Ok(result)
    }
}
pub fn safe_id(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}
pub fn safe_file(s: &str) -> bool {
    !s.is_empty()
        && s != "."
        && s != ".."
        && !s.ends_with(['.', ' '])
        && !s.contains(['/', '\\', ':'])
        && std::path::Path::new(s).file_name().is_some_and(|n| n == s)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn migrates_existing_files_without_renaming() {
        let old = serde_json::json!({"id":"Q8_0","name":"test","quantization":"Q8_0","modelFile":"a.gguf","mmprojFile":"b.gguf","modelSize":4,"mmprojSize":8,"modelSha256":"a".repeat(64),"mmprojSha256":"b".repeat(64),"installed":true,"imported":false});
        let m = ModelManifest::from_slice(&serde_json::to_vec(&old).unwrap()).unwrap();
        assert_eq!(m.file("main").unwrap().path, "a.gguf");
        assert_eq!(m.runtime, "llamacpp");
        assert!(m.installed);
        crate::engine::RuntimeRegistry::default()
            .validate_manifest(&m)
            .unwrap();
        let mut bad = m;
        bad.runtime = "onnx".into();
        assert!(
            crate::engine::RuntimeRegistry::default()
                .validate_manifest(&bad)
                .is_err()
        );
    }
}

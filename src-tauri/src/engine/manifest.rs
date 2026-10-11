//! Typed identities for the mixed-runtime installation and its private download sources.
use serde::Serialize;
use std::path::{Path, PathBuf};

/// Processing role, independent of the model's storage format.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum ModelPurpose {
    Asr,
    Vad,
}

/// Native artifact format; each role retains its own runtime.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum ModelFormat {
    Gguf,
    Onnx,
}

/// Public identity with no native paths or provisioning details.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelDescriptor {
    pub id: &'static str,
    pub name: &'static str,
    pub version: &'static str,
    pub purpose: ModelPurpose,
    pub format: ModelFormat,
    pub quantization: Option<&'static str>,
}

/// Backend-only source and destination for one checksum-verified artifact.
pub(crate) struct ModelResource {
    pub descriptor: ModelDescriptor,
    pub repository: &'static str,
    pub revision: &'static str,
    pub remote_file: &'static str,
    pub directory: &'static str,
    pub filename: &'static str,
    pub sha256: &'static str,
}

impl ModelResource {
    /// Resolves only compile-time relative destinations inside the native model folder.
    pub(crate) fn destination(&self, base: &Path) -> PathBuf {
        base.join(self.directory).join(self.filename)
    }
}

/// Immutable Handy export compatible with the pinned transcribe.cpp runtime.
pub(crate) const PARAKEET: ModelResource = ModelResource {
    descriptor: ModelDescriptor {
        id: "parakeet-tdt-0.6b-v3",
        name: "Parakeet TDT 0.6B",
        version: "v3",
        purpose: ModelPurpose::Asr,
        format: ModelFormat::Gguf,
        quantization: Some("Q8_0"),
    },
    repository: "handy-computer/parakeet-tdt-0.6b-v3-gguf",
    revision: "90f082450fcbacdb54e5900c44ef697c9ea59622",
    remote_file: "parakeet-tdt-0.6b-v3-Q8_0.gguf",
    directory: "parakeet",
    filename: "parakeet-tdt-0.6b-v3-Q8_0.gguf",
    sha256: "5859f77944efcd8eafa23a6350731960b2b55b2203df51f319665c807d802cc7",
};

/// Existing Silero source is retained; the trusted digest pins its content on main.
pub(crate) const SILERO: ModelResource = ModelResource {
    descriptor: ModelDescriptor {
        id: "silero-vad-v5.1",
        name: "Silero",
        version: "v5.1",
        purpose: ModelPurpose::Vad,
        format: ModelFormat::Onnx,
        quantization: None,
    },
    repository: "Derur/silero-models",
    revision: "main",
    remote_file: "vad/silero_vad.onnx",
    directory: "",
    filename: "silero_vad.onnx",
    sha256: "2623a2953f6ff3d2c1e61740c6cdb7168133479b267dfef114a4a3cc5bdd788f",
};

/// Required artifacts in stable download and display order.
pub(crate) const MODELS: &[ModelResource] = &[PARAKEET, SILERO];

/// Projects private source metadata into safe, serializable UI identities.
pub(crate) fn descriptors() -> Vec<ModelDescriptor> {
    MODELS
        .iter()
        .map(|model| model.descriptor.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Locks runtime roles, safe destinations, unique identities and the public wire shape.
    #[test]
    fn manifest_and_public_descriptors() -> anyhow::Result<()> {
        let base = Path::new("/models");
        assert_eq!(
            PARAKEET.destination(base),
            base.join("parakeet").join(PARAKEET.filename)
        );
        assert_eq!(SILERO.destination(base), base.join("silero_vad.onnx"));
        assert_ne!(PARAKEET.descriptor.id, SILERO.descriptor.id);
        assert_eq!(PARAKEET.revision.len(), 40);
        for model in MODELS {
            assert_eq!(model.sha256.len(), 64);
            assert!(model.sha256.bytes().all(|value| value.is_ascii_hexdigit()));
            assert!(!Path::new(model.directory).is_absolute());
            assert!(!model.directory.split('/').any(|part| part == ".."));
        }
        assert_eq!(
            serde_json::to_value(descriptors())?,
            serde_json::json!([
                {"id":"parakeet-tdt-0.6b-v3","name":"Parakeet TDT 0.6B","version":"v3","purpose":"ASR","format":"GGUF","quantization":"Q8_0"},
                {"id":"silero-vad-v5.1","name":"Silero","version":"v5.1","purpose":"VAD","format":"ONNX","quantization":null}
            ])
        );
        Ok(())
    }
}

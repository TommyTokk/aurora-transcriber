//! Typed identities for the mixed-runtime installation and its private download sources.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Persisted ASR identity; only catalogued models can cross the IPC boundary.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AsrModelId {
    #[default]
    #[serde(rename = "parakeet-tdt-0.6b-v3")]
    Parakeet,
    #[serde(rename = "whisper-large-v3-turbo")]
    WhisperTurbo,
}
impl AsrModelId {
    /// Resolves the immutable artifact and native contract for this ASR choice.
    pub(crate) fn resource(self) -> &'static ModelResource {
        match self {
            Self::Parakeet => &PARAKEET,
            Self::WhisperTurbo => &WHISPER_TURBO,
        }
    }
    /// Returns the exact native architecture and variant expected from the GGUF.
    pub(crate) fn contract(self) -> (&'static str, &'static str) {
        match self {
            Self::Parakeet => ("parakeet", "tdt-0.6b-v3"),
            Self::WhisperTurbo => ("whisper", "whisper-large-v3-turbo"),
        }
    }
}

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
    pub size_bytes: u64,
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
        size_bytes: 739_508_576,
    },
    repository: "handy-computer/parakeet-tdt-0.6b-v3-gguf",
    revision: "90f082450fcbacdb54e5900c44ef697c9ea59622",
    remote_file: "parakeet-tdt-0.6b-v3-Q8_0.gguf",
    directory: "parakeet",
    filename: "parakeet-tdt-0.6b-v3-Q8_0.gguf",
    sha256: "5859f77944efcd8eafa23a6350731960b2b55b2203df51f319665c807d802cc7",
};

/// Whisper Turbo export published by the transcribe.cpp maintainers, pinned by content.
pub(crate) const WHISPER_TURBO: ModelResource = ModelResource {
    descriptor: ModelDescriptor {
        id: "whisper-large-v3-turbo",
        name: "Whisper Turbo",
        version: "large-v3-turbo",
        purpose: ModelPurpose::Asr,
        format: ModelFormat::Gguf,
        quantization: Some("Q8_0"),
        size_bytes: 886_381_728,
    },
    repository: "handy-computer/whisper-large-v3-turbo-gguf",
    revision: "18ada752005f5fddef499256f9a458d0b37a203a",
    remote_file: "whisper-large-v3-turbo-Q8_0.gguf",
    directory: "whisper",
    filename: "whisper-large-v3-turbo-Q8_0.gguf",
    sha256: "257aa7ec9121c22c167d5e2c49e3e76b4ba0e6e27ebc13074eba0a80f8190220",
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
        size_bytes: 2_327_524,
    },
    repository: "Derur/silero-models",
    revision: "main",
    remote_file: "vad/silero_vad.onnx",
    directory: "",
    filename: "silero_vad.onnx",
    sha256: "2623a2953f6ff3d2c1e61740c6cdb7168133479b267dfef114a4a3cc5bdd788f",
};

/// Available artifacts in stable display order; availability does not imply a requirement.
pub(crate) const MODELS: &[ModelResource] = &[PARAKEET, WHISPER_TURBO, SILERO];

/// Returns only the selected ASR and the shared VAD dependency.
pub(crate) fn required(model: AsrModelId) -> [&'static ModelResource; 2] {
    [model.resource(), &SILERO]
}

/// Rejects arbitrary paths and unknown identities before any filesystem action.
pub(crate) fn find(id: &str) -> Result<&'static ModelResource, String> {
    MODELS
        .iter()
        .find(|model| model.descriptor.id == id)
        .ok_or_else(|| "Unknown model identity.".into())
}

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
                {"id":"parakeet-tdt-0.6b-v3","name":"Parakeet TDT 0.6B","version":"v3","purpose":"ASR","format":"GGUF","quantization":"Q8_0","sizeBytes":739508576},
                {"id":"whisper-large-v3-turbo","name":"Whisper Turbo","version":"large-v3-turbo","purpose":"ASR","format":"GGUF","quantization":"Q8_0","sizeBytes":886381728},
                {"id":"silero-vad-v5.1","name":"Silero","version":"v5.1","purpose":"VAD","format":"ONNX","quantization":null,"sizeBytes":2327524}
            ])
        );
        Ok(())
    }
}

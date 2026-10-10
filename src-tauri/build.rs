use sha2::{Digest, Sha256};
/// Verifies every pinned runtime resource before generating Tauri permissions and bundles.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var("CARGO_CFG_TARGET_OS")? == "macos" {
        let directory = std::path::Path::new("resources/onnxruntime");
        let manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(directory.join("manifest.json"))?)?;
        let files = manifest["files"]
            .as_object()
            .ok_or("Invalid runtime manifest")?;
        for (name, expected) in files {
            let path = directory.join(name);
            println!("cargo:rerun-if-changed={}", path.display());
            let actual = format!("{:x}", Sha256::digest(std::fs::read(&path)?));
            if Some(actual.as_str()) != expected.as_str() {
                return Err(format!("Bundled runtime checksum mismatch: {name}").into());
            }
        }
        println!("cargo:rerun-if-changed=resources/onnxruntime/manifest.json");
    }
    tauri_build::build();
    Ok(())
}

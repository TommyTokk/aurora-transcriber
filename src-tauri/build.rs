use sha2::{Digest, Sha256};
/// Verifies every pinned runtime resource before generating Tauri permissions and bundles.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var("CARGO_CFG_TARGET_OS")? == "macos" {
        link_macos_availability()?;
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

/// Links Apple's availability helpers used by statically embedded Objective-C Metal code.
///
/// Rust's linker uses nodefaultlibs, so Clang does not automatically add the
/// compiler runtime supplying __isPlatformVersionAtLeast for older macOS targets.
fn link_macos_availability() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::process::Command::new("xcrun")
        .args(["clang", "--print-resource-dir"])
        .output()?;
    if !output.status.success() {
        return Err("Apple Clang resource directory is unavailable".into());
    }
    let directory =
        std::path::PathBuf::from(String::from_utf8(output.stdout)?.trim()).join("lib/darwin");
    if !directory.join("libclang_rt.osx.a").is_file() {
        return Err("Apple Clang macOS compiler runtime is unavailable".into());
    }
    println!("cargo:rustc-link-search=native={}", directory.display());
    println!("cargo:rustc-link-lib=static=clang_rt.osx");
    Ok(())
}
